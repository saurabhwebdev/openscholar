use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::class as class_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/classes", get(list_classes).post(create_class_submit))
        .route("/classes/new", get(new_class_page))
        .route("/classes/{id}", get(class_detail_page))
        .route("/classes/{id}/enroll", post(enroll_submit))
        .route("/classes/{id}/unenroll/{student_id}", post(unenroll_submit))
        .route("/classes/{id}/edit", get(edit_class_page))
        .route("/classes/{id}/update", post(update_class_submit))
        .route("/classes/{id}/delete", post(delete_class_submit))
        .route("/school-years", post(create_year_submit))
        .route("/school-years/{id}/set-current", post(set_current_submit))
        .route("/school-years/{id}/delete", post(delete_year_submit))
}

// ---------------------------------------------------------------------------
// Shared presentation helpers
// ---------------------------------------------------------------------------

fn user_ctx(user: &CurrentUser) -> (String, String) {
    let initial = user
        .full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    (user.full_name.clone(), initial)
}

fn today() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn can_mutate(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Staff])
}

fn check_mutate(user: &CurrentUser) -> Result<(), AppError> {
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Staff])
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

fn db_flash(e: AppError) -> (String, String) {
    match e {
        AppError::BadRequest(msg) => (msg, "error".to_string()),
        AppError::NotFound => ("Not found".to_string(), "error".to_string()),
        AppError::Db(sqlx::Error::Database(db)) if db.is_unique_violation() => (
            "That record already exists".to_string(),
            "error".to_string(),
        ),
        other => {
            tracing::error!(error = %other, "classes route error");
            ("Something went wrong".to_string(), "error".to_string())
        }
    }
}

fn opt_text(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn opt_int(v: Option<String>) -> Result<Option<i64>, AppError> {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("Invalid selection".to_string())),
    }
}

fn opt_capacity(v: Option<String>) -> Result<Option<i32>, AppError> {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => s
            .parse::<i32>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("Capacity must be a number".to_string())),
    }
}

/// Template-friendly year option (Askama can't compare `Option`s inline).
struct YearOption {
    id: i64,
    name: String,
    is_current: bool,
    selected: bool,
}

fn year_options(years: &[class_model::SchoolYearRow], selected: Option<i64>) -> Vec<YearOption> {
    years
        .iter()
        .map(|y| YearOption {
            id: y.id,
            name: y.name.clone(),
            is_current: y.is_current,
            selected: Some(y.id) == selected,
        })
        .collect()
}

/// Template-friendly teacher option.
struct TeacherOptionView {
    id: i64,
    full_name: String,
    selected: bool,
}

fn teacher_options(
    teachers: &[class_model::TeacherOption],
    selected: Option<i64>,
) -> Vec<TeacherOptionView> {
    teachers
        .iter()
        .map(|t| TeacherOptionView {
            id: t.id,
            full_name: t.full_name.clone(),
            selected: Some(t.id) == selected,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GET /classes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListParams {
    // Lenient strings: garbage falls back to defaults instead of Axum's
    // bare 400 rejection page.
    year: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "classes/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    years: Vec<YearOption>,
    selected_year: Option<i64>,
    classes: Vec<class_model::ClassView>,
    can_mutate: bool,
}

async fn list_classes(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    let years = class_model::list_years(&state.pool).await?;
    let selected_year = params
        .year
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| {
            years
                .iter()
                .find(|y| y.is_current)
                .map(|y| y.id)
                .or_else(|| years.first().map(|y| y.id))
        });
    let classes = match selected_year {
        Some(y) => class_model::list_classes(&state.pool, y).await?,
        None => Vec::new(),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "classes",
        section_label: "Classes",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        years: year_options(&years, selected_year),
        selected_year,
        classes,
        can_mutate: can_mutate(&user),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// School years
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct YearForm {
    name: String,
    start_date: String,
    end_date: String,
}

async fn create_year_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<YearForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let start = NaiveDate::parse_from_str(form.start_date.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("Start date must be YYYY-MM-DD".to_string()))?;
    let end = NaiveDate::parse_from_str(form.end_date.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("End date must be YYYY-MM-DD".to_string()))?;
    if form.name.trim().is_empty() {
        return Ok(flash_redirect("/classes", "Year name is required", "error"));
    }
    if end < start {
        return Ok(flash_redirect(
            "/classes",
            "End date must be on or after the start date",
            "error",
        ));
    }
    match class_model::create_year(&state.pool, form.name.trim(), start, end).await {
        Ok(y) => Ok(flash_redirect(
            &format!("/classes?year={}", y.id),
            "School year created",
            "ok",
        )),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/classes", &msg, &kind))
        }
    }
}

async fn set_current_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    match class_model::set_current_year(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect(
            &format!("/classes?year={id}"),
            "Current year updated",
            "ok",
        )),
        Err(AppError::NotFound) => Ok(flash_redirect("/classes", "Not found", "error")),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/classes", &msg, &kind))
        }
    }
}

async fn delete_class_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    // Admin-only: deleting a class cascades its enrollments and grades.
    crate::auth::middleware::require_roles(&user, &[Role::Admin])?;
    // NotFound still propagates as a real 404; the history guard becomes a flash.
    match class_model::delete_class(&state.pool, id).await {
        Ok(year_id) => Ok(flash_redirect(
            &format!("/classes?year={year_id}"),
            "Class deleted",
            "ok",
        )),
        Err(AppError::BadRequest(msg)) => {
            Ok(flash_redirect(&format!("/classes/{id}"), &msg, "error"))
        }
        Err(e) => Err(e),
    }
}

async fn delete_year_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    // Admin-only: deleting a school year is a bulk destructive operation.
    crate::auth::middleware::require_roles(&user, &[Role::Admin])?;
    let year = match class_model::year_row(&state.pool, id).await {
        Ok(y) => y,
        Err(AppError::NotFound) => return Ok(flash_redirect("/classes", "Not found", "error")),
        Err(e) => return Err(e),
    };
    let class_count = class_model::count_classes_in_year(&state.pool, id).await?;
    if let Err(msg) = class_model::can_delete_year(year.is_current, class_count) {
        return Ok(flash_redirect(&format!("/classes?year={id}"), msg, "error"));
    }
    match class_model::delete_year(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect("/classes", "School year deleted", "ok")),
        Err(AppError::NotFound) => Ok(flash_redirect("/classes", "Not found", "error")),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// New / create / edit / update
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct NewParams {
    year: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "classes/new.html")]
struct FormTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    editing: bool,
    class_id: Option<i64>,
    years: Vec<YearOption>,
    teachers: Vec<TeacherOptionView>,
    name: String,
    grade_level: String,
    room: String,
    capacity: String,
}

async fn new_class_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<NewParams>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let years = class_model::list_years(&state.pool).await?;
    let selected_year = params
        .year
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| {
            years
                .iter()
                .find(|y| y.is_current)
                .map(|y| y.id)
                .or_else(|| years.first().map(|y| y.id))
        });
    if years.is_empty() || selected_year.is_none() {
        return Ok(
            flash_redirect("/classes", "Create a school year first", "error").into_response(),
        );
    }
    let teachers = class_model::list_teacher_options(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = FormTemplate {
        section: "classes",
        section_label: "Classes",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        editing: false,
        class_id: None,
        years: year_options(&years, selected_year),
        teachers: teacher_options(&teachers, None),
        name: String::new(),
        grade_level: String::new(),
        room: String::new(),
        capacity: String::new(),
    };
    t.render()
        .map(axum::response::Html)
        .map_err(AppError::from)
        .map(|r| r.into_response())
}

#[derive(Debug, Deserialize)]
struct ClassForm {
    school_year_id: Option<String>,
    name: String,
    grade_level: Option<String>,
    room: Option<String>,
    teacher_id: Option<String>,
    capacity: Option<String>,
}

async fn create_class_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ClassForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let year_id = opt_int(form.school_year_id)?;
    let capacity = opt_capacity(form.capacity)?;
    let teacher_id = opt_int(form.teacher_id)?;
    if let Err(msg) = class_model::validate_class(&form.name, year_id, capacity) {
        return Ok(flash_redirect("/classes/new", &msg, "error").into_response());
    }
    let year_id = year_id.expect("validated");
    match class_model::create_class(
        &state.pool,
        year_id,
        form.name.trim(),
        opt_text(form.grade_level).as_deref(),
        opt_text(form.room).as_deref(),
        teacher_id,
        capacity,
    )
    .await
    {
        Ok(row) => Ok(Redirect::to(&format!("/classes/{}", row.id)).into_response()),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/classes/new", &msg, &kind).into_response())
        }
    }
}

#[derive(Debug, Deserialize)]
struct EditParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

async fn edit_class_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<EditParams>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let row = class_model::class_row(&state.pool, id).await?;
    let years = class_model::list_years(&state.pool).await?;
    let teachers = class_model::list_teacher_options(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = FormTemplate {
        section: "classes",
        section_label: "Classes",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        editing: true,
        class_id: Some(row.id),
        years: year_options(&years, Some(row.school_year_id)),
        teachers: teacher_options(&teachers, row.teacher_id),
        name: row.name,
        grade_level: row.grade_level.unwrap_or_default(),
        room: row.room.unwrap_or_default(),
        capacity: row.capacity.map(|c| c.to_string()).unwrap_or_default(),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

async fn update_class_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<ClassForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let year_id = opt_int(form.school_year_id)?;
    let capacity = opt_capacity(form.capacity)?;
    let teacher_id = opt_int(form.teacher_id)?;
    if let Err(msg) = class_model::validate_class(&form.name, year_id, capacity) {
        return Ok(flash_redirect(
            &format!("/classes/{id}/edit"),
            &msg,
            "error",
        ));
    }
    let year_id = year_id.expect("validated");
    match class_model::update_class(
        &state.pool,
        id,
        year_id,
        form.name.trim(),
        opt_text(form.grade_level).as_deref(),
        opt_text(form.room).as_deref(),
        teacher_id,
        capacity,
    )
    .await
    {
        Ok(_) => Ok(flash_redirect(
            &format!("/classes/{id}"),
            "Class updated",
            "ok",
        )),
        Err(AppError::NotFound) => Ok(flash_redirect("/classes", "Not found", "error")),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect(&format!("/classes/{id}/edit"), &msg, &kind))
        }
    }
}

// ---------------------------------------------------------------------------
// Detail + enrollment
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DetailParams {
    q: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "classes/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    class: class_model::ClassView,
    roster: Vec<class_model::RosterEntry>,
    candidates: Vec<class_model::AvailableStudent>,
    q: String,
    can_mutate: bool,
    is_full: bool,
}

async fn class_detail_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<DetailParams>,
) -> Result<impl IntoResponse, AppError> {
    let class = class_model::class_detail(&state.pool, id).await?;
    let roster = class_model::list_roster(&state.pool, id).await?;
    let q = params.q.unwrap_or_default();
    let candidates = class_model::available_students_for(&state.pool, id, &q).await?;
    let is_full = class
        .capacity
        .is_some_and(|c| roster.len() as i64 >= c as i64);
    let (user_name, user_initial) = user_ctx(&user);
    let t = DetailTemplate {
        section: "classes",
        section_label: "Classes",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        class,
        roster,
        candidates,
        q,
        can_mutate: can_mutate(&user),
        is_full,
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct EnrollForm {
    student_id: i64,
}

async fn enroll_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<EnrollForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    match class_model::enroll(&state.pool, id, form.student_id).await {
        Ok(()) => Ok(flash_redirect(
            &format!("/classes/{id}"),
            "Student enrolled",
            "ok",
        )),
        Err(AppError::BadRequest(msg)) => {
            Ok(flash_redirect(&format!("/classes/{id}"), &msg, "error"))
        }
        Err(AppError::NotFound) => Ok(flash_redirect("/classes", "Not found", "error")),
        Err(e) => Err(e),
    }
}

async fn unenroll_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, student_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    // Friendly 404 if the class itself is gone; removing a non-member is a no-op.
    class_model::class_row(&state.pool, id).await?;
    class_model::unenroll(&state.pool, id, student_id).await?;
    Ok(flash_redirect(
        &format!("/classes/{id}"),
        "Student removed",
        "ok",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn location(r: Redirect) -> (StatusCode, String) {
        let resp = r.into_response();
        let status = resp.status();
        let loc = resp
            .headers()
            .get("location")
            .expect("redirect has location")
            .to_str()
            .expect("location is ascii")
            .to_string();
        (status, loc)
    }

    #[test]
    fn flash_redirect_encodes_message_on_plain_base() {
        let (status, loc) = location(flash_redirect("/classes", "Class deleted", "ok"));
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(loc, "/classes?flash=Class+deleted&flash_kind=ok");
    }

    #[test]
    fn flash_redirect_appends_with_ampersand_when_query_present() {
        let (status, loc) = location(flash_redirect(
            "/classes?year=7",
            "School year deleted",
            "ok",
        ));
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(
            loc,
            "/classes?year=7&flash=School+year+deleted&flash_kind=ok"
        );
    }

    #[test]
    fn flash_redirect_carries_error_kind_for_guard_refusal() {
        let (_, loc) = location(flash_redirect(
            "/classes?year=7",
            "The current school year cannot be deleted",
            "error",
        ));
        assert_eq!(
            loc,
            "/classes?year=7&flash=The+current+school+year+cannot+be+deleted&flash_kind=error"
        );
    }
}
