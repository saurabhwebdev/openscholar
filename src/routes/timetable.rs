use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use chrono::Datelike;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::class as class_model;
use crate::models::timetable as tt;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/subjects", get(subjects_page).post(subject_create))
        .route("/subjects/{id}/delete", post(subject_delete))
        .route("/timetable", get(timetable_page))
        .route("/timetable/slots", post(slot_create))
        .route("/timetable/slots/{id}/delete", post(slot_delete))
        .route("/timetable/teacher", get(teacher_timetable_page))
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
            tracing::error!(error = %other, "timetable route error");
            ("Something went wrong".to_string(), "error".to_string())
        }
    }
}

fn opt_text(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn opt_id(v: Option<String>) -> Result<Option<i64>, AppError> {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("Invalid selection".to_string())),
    }
}

// ---------------------------------------------------------------------------
// Template-friendly option views (Askama can't compare `Option`s inline)
// ---------------------------------------------------------------------------

struct ClassOption {
    id: i64,
    name: String,
    selected: bool,
}

struct SubjectOption {
    id: i64,
    label: String,
}

struct TeacherOption {
    id: i64,
    full_name: String,
    selected: bool,
}

struct PeriodOption {
    id: i64,
    label: String,
}

fn period_options(periods: &[tt::PeriodRow]) -> Vec<PeriodOption> {
    let mut sorted = periods.to_vec();
    sorted.sort_by(|a, b| {
        a.weekday
            .cmp(&b.weekday)
            .then_with(|| a.start.cmp(&b.start))
    });
    sorted
        .into_iter()
        .map(|p| PeriodOption {
            id: p.id,
            label: format!(
                "{} {} · {}–{}",
                tt::weekday_short(p.weekday),
                p.label,
                p.start,
                p.end
            ),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GET /subjects — list + inline create (admin/staff write, all roles read)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FlashParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct SubjectView {
    id: i64,
    code: String,
    name: String,
    department: String,
    in_use: bool,
}

#[derive(Template)]
#[template(path = "subjects/list.html")]
struct SubjectsTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    subjects: Vec<SubjectView>,
    can_mutate: bool,
}

async fn subjects_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<FlashParams>,
) -> Result<impl IntoResponse, AppError> {
    let subjects = tt::list_subjects(&state.pool).await?;
    let assigned = tt::assigned_subject_ids(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = SubjectsTemplate {
        section: "subjects",
        section_label: "Subjects",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        subjects: subjects
            .into_iter()
            .map(|s| SubjectView {
                id: s.id,
                code: s.code,
                name: s.name,
                department: s.department.unwrap_or_else(|| "—".to_string()),
                in_use: assigned.contains(&s.id),
            })
            .collect(),
        can_mutate: can_mutate(&user),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct SubjectForm {
    code: String,
    name: String,
    department: Option<String>,
}

async fn subject_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<SubjectForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    if let Err(msg) = tt::validate_subject(&form.code, &form.name) {
        return Ok(flash_redirect("/subjects", &msg, "error"));
    }
    match tt::create_subject(
        &state.pool,
        form.code.trim(),
        form.name.trim(),
        opt_text(form.department).as_deref(),
    )
    .await
    {
        Ok(_) => Ok(flash_redirect("/subjects", "Subject created", "ok")),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/subjects", &msg, &kind))
        }
    }
}

async fn subject_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    match tt::delete_subject(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect("/subjects", "Subject deleted", "ok")),
        Err(AppError::NotFound) => Ok(flash_redirect("/subjects", "Not found", "error")),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect("/subjects", &msg, "error")),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// GET /timetable — class grid + assign form (any authed role reads)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GridParams {
    // Free-form string parsed leniently: empty/garbage falls back to the
    // default class instead of Axum's bare 400 rejection page.
    class_id: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "timetable/grid.html")]
struct GridTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    classes: Vec<ClassOption>,
    selected_class: Option<i64>,
    class_name: String,
    has_class: bool,
    today_col: usize,
    rows: Vec<tt::GridRow>,
    subjects: Vec<SubjectOption>,
    teachers: Vec<TeacherOption>,
    periods: Vec<PeriodOption>,
    assigned: Vec<tt::ClassSubjectView>,
    can_mutate: bool,
}

async fn timetable_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<GridParams>,
) -> Result<impl IntoResponse, AppError> {
    // Default to the first class of the current year.
    let classes = match class_model::current_year(&state.pool).await? {
        Some(y) => class_model::list_classes(&state.pool, y.id).await?,
        None => Vec::new(),
    };
    let selected = params
        .class_id
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| classes.first().map(|c| c.id));
    let class_name = classes
        .iter()
        .find(|c| Some(c.id) == selected)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let (rows, subjects, teachers, periods, assigned) = match selected {
        Some(class_id) => {
            let slots = tt::class_grid(&state.pool, class_id).await?;
            let all_periods = tt::list_periods(&state.pool).await?;
            let rows = tt::build_grid(&all_periods, &slots);
            let subjects = tt::list_subjects(&state.pool)
                .await?
                .into_iter()
                .map(|s| SubjectOption {
                    id: s.id,
                    label: format!("{} — {}", s.code, s.name),
                })
                .collect();
            let teachers = class_model::list_teacher_options(&state.pool)
                .await?
                .into_iter()
                .map(|t| TeacherOption {
                    id: t.id,
                    full_name: t.full_name,
                    selected: false,
                })
                .collect();
            let periods = period_options(&all_periods);
            let assigned = tt::list_class_subjects(&state.pool, class_id).await?;
            (rows, subjects, teachers, periods, assigned)
        }
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let t = GridTemplate {
        section: "timetable",
        section_label: "Timetable",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        classes: classes
            .into_iter()
            .map(|c| ClassOption {
                id: c.id,
                name: c.name,
                selected: Some(c.id) == selected,
            })
            .collect(),
        selected_class: selected,
        class_name,
        has_class: selected.is_some(),
        today_col: match chrono::Local::now().weekday().number_from_monday() {
            1..=5 => (chrono::Local::now().weekday().number_from_monday() - 1) as usize,
            _ => 99,
        },
        rows,
        subjects,
        teachers,
        periods,
        assigned,
        can_mutate: can_mutate(&user),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct SlotForm {
    class_id: i64,
    subject_id: Option<String>,
    period_id: Option<String>,
    teacher_id: Option<String>,
    room: Option<String>,
}

async fn slot_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<SlotForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let back = format!("/timetable?class_id={}", form.class_id);
    let subject_id = opt_id(form.subject_id)?;
    let period_id = opt_id(form.period_id)?;
    let teacher_id = opt_id(form.teacher_id)?;
    let (Some(subject_id), Some(period_id)) = (subject_id, period_id) else {
        return Ok(flash_redirect(
            &back,
            "Choose a subject and a period",
            "error",
        ));
    };
    match tt::save_slot(
        &state.pool,
        form.class_id,
        subject_id,
        period_id,
        teacher_id,
        opt_text(form.room).as_deref(),
    )
    .await
    {
        Ok(_) => {
            // Keep the class↔subject link in sync for the assigned list.
            let _ = tt::assign_subject(&state.pool, form.class_id, subject_id, teacher_id).await;
            Ok(flash_redirect(&back, "Lesson booked", "ok"))
        }
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(&back, &msg, "error")),
        Err(e) => Err(e),
    }
}

async fn slot_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    match tt::delete_slot(&state.pool, id).await {
        Ok(class_id) => Ok(flash_redirect(
            &format!("/timetable?class_id={class_id}"),
            "Lesson removed",
            "ok",
        )),
        Err(AppError::NotFound) => Ok(flash_redirect("/timetable", "Not found", "error")),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// GET /timetable/teacher — weekly view per teacher (any authed role reads)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TeacherGridParams {
    staff_id: Option<i64>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "timetable/teacher.html")]
struct TeacherGridTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    teachers: Vec<TeacherOption>,
    staff_name: String,
    has_staff: bool,
    rows: Vec<tt::GridRow>,
}

async fn teacher_timetable_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<TeacherGridParams>,
) -> Result<impl IntoResponse, AppError> {
    let options = class_model::list_teacher_options(&state.pool).await?;
    let selected = params.staff_id.or_else(|| options.first().map(|t| t.id));
    let staff_name = options
        .iter()
        .find(|t| Some(t.id) == selected)
        .map(|t| t.full_name.clone())
        .unwrap_or_default();
    let rows = match selected {
        Some(staff_id) => {
            let slots = tt::teacher_grid(&state.pool, staff_id).await?;
            let all_periods = tt::list_periods(&state.pool).await?;
            tt::build_grid(&all_periods, &slots)
        }
        None => Vec::new(),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let t = TeacherGridTemplate {
        section: "timetable",
        section_label: "Timetable",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        teachers: options
            .into_iter()
            .map(|o| TeacherOption {
                id: o.id,
                full_name: o.full_name,
                selected: Some(o.id) == selected,
            })
            .collect(),
        staff_name,
        has_staff: selected.is_some(),
        rows,
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_options_label_reads_naturally() {
        let periods = vec![tt::PeriodRow {
            id: 3,
            weekday: 0,
            start: "08:00".to_string(),
            end: "08:45".to_string(),
            label: "P1".to_string(),
        }];
        let opts = period_options(&periods);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].id, 3);
        assert_eq!(opts[0].label, "Mon P1 · 08:00–08:45");
    }

    #[test]
    fn period_options_sort_weekday_then_time() {
        let periods = vec![
            tt::PeriodRow {
                id: 2,
                weekday: 1,
                start: "08:00".to_string(),
                end: "08:45".to_string(),
                label: "P1".to_string(),
            },
            tt::PeriodRow {
                id: 1,
                weekday: 0,
                start: "08:45".to_string(),
                end: "09:30".to_string(),
                label: "P2".to_string(),
            },
        ];
        let opts = period_options(&periods);
        assert_eq!(opts[0].id, 1);
        assert_eq!(opts[1].id, 2);
    }
}
