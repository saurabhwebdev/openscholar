use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::class as class_model;
use crate::models::settings as settings_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(settings_page))
        .route("/settings/profile", post(profile_submit))
        .route("/settings/scales", post(scale_create))
        .route("/settings/scales/{id}/delete", post(scale_delete))
        .route("/settings/semesters", post(semester_create))
        .route(
            "/settings/semesters/{id}/set-current",
            post(semester_set_current),
        )
        .route("/settings/semesters/{id}/delete", post(semester_delete))
        .route("/settings/raw", post(raw_submit))
        .route("/settings/password", post(password_submit))
}

#[derive(Debug, Deserialize)]
struct PasswordForm {
    #[serde(default)]
    current_password: String,
    #[serde(default)]
    new_password: String,
}

/// Any signed-in user changes their OWN password. Requires the current
/// password, and signs out the user's other sessions on success.
async fn password_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    headers: axum::http::HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    let current_hash = crate::models::user::password_hash_for(&state.pool, user.id)
        .await?
        .ok_or(AppError::NotFound)?;
    if crate::auth::password::verify_password(&form.current_password, &current_hash).is_err() {
        return Ok(
            flash_redirect("/settings", "Current password is incorrect", "error").into_response(),
        );
    }
    let new_hash = match crate::auth::password::hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => {
            return Ok(flash_redirect(
                "/settings",
                "New password must be at least 8 characters",
                "error",
            )
            .into_response());
        }
    };
    crate::models::user::update_password(&state.pool, user.id, &new_hash).await?;
    // Keep the acting session; invalidate the user's other sessions.
    if let Some(sid) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(crate::auth::session::session_id_from_cookie_header)
    {
        crate::auth::session::delete_other_sessions(&state.pool, user.id, sid).await?;
    }
    Ok(flash_redirect("/settings", "Password changed", "ok").into_response())
}

fn today() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn user_ctx(user: &CurrentUser) -> (String, String) {
    let initial = user
        .full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    (user.full_name.clone(), initial)
}

fn check_admin(user: &CurrentUser) -> Result<(), AppError> {
    require_roles(user, &[Role::Admin])
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
            tracing::error!(error = %other, "settings route error");
            ("Something went wrong".to_string(), "error".to_string())
        }
    }
}

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

#[derive(Debug, Clone)]
struct SemesterView {
    id: i64,
    year_name: String,
    name: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    is_current: bool,
}

#[derive(Debug, Deserialize)]
struct ListParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "settings/list.html")]
struct SettingsTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    school_name: String,
    school_address: String,
    school_email: String,
    school_phone: String,
    scales: Vec<settings_model::GradingScaleRow>,
    years: Vec<YearOption>,
    semesters: Vec<SemesterView>,
    settings: Vec<settings_model::SettingRow>,
}

async fn settings_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    check_admin(&user)?;
    let years = class_model::list_years(&state.pool).await?;
    let scales = settings_model::list_scales(&state.pool).await?;
    let sem_rows = settings_model::list_all_semesters(&state.pool).await?;
    let settings = settings_model::list_settings(&state.pool).await?;

    let year_name = |id: i64| -> String {
        years
            .iter()
            .find(|y| y.id == id)
            .map(|y| y.name.clone())
            .unwrap_or_default()
    };
    let semesters: Vec<SemesterView> = sem_rows
        .into_iter()
        .map(|s| SemesterView {
            id: s.id,
            year_name: year_name(s.school_year_id),
            name: s.name,
            start_date: s.start_date,
            end_date: s.end_date,
            is_current: s.is_current,
        })
        .collect();

    let get_val = |key: &str| -> String {
        settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.clone())
            .unwrap_or_default()
    };
    // also check db for profile keys not in list? list covers all
    let school_name = get_val("school_name");
    let school_address = get_val("school_address");
    let school_email = get_val("school_email");
    let school_phone = get_val("school_phone");

    let selected_year = years
        .iter()
        .find(|y| y.is_current)
        .map(|y| y.id)
        .or_else(|| years.first().map(|y| y.id));
    let (user_name, user_initial) = user_ctx(&user);
    let t = SettingsTemplate {
        section: "settings",
        section_label: "Settings",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        school_name,
        school_address,
        school_email,
        school_phone,
        scales,
        years: year_options(&years, selected_year),
        semesters,
        settings,
    };
    t.render().map(Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct ProfileForm {
    school_name: Option<String>,
    school_address: Option<String>,
    school_email: Option<String>,
    school_phone: Option<String>,
}

async fn profile_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ProfileForm>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    let pairs = [
        ("school_name", form.school_name),
        ("school_address", form.school_address),
        ("school_email", form.school_email),
        ("school_phone", form.school_phone),
    ];
    for (k, v) in pairs {
        if let Some(val) = v {
            let val = val.trim().to_string();
            settings_model::set_setting(&state.pool, k, &val).await?;
        }
    }
    Ok(flash_redirect("/settings", "School profile saved", "ok").into_response())
}

#[derive(Debug, Deserialize)]
struct ScaleForm {
    name: String,
    min_score: String,
    max_score: String,
    grade: String,
    points: Option<String>,
}

async fn scale_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ScaleForm>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    let min: f64 = match form.min_score.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                flash_redirect("/settings", "Minimum score must be a number", "error")
                    .into_response(),
            );
        }
    };
    let max: f64 = match form.max_score.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                flash_redirect("/settings", "Maximum score must be a number", "error")
                    .into_response(),
            );
        }
    };
    if let Err(msg) = settings_model::validate_scale(&form.name, min, max, &form.grade) {
        return Ok(flash_redirect("/settings", &msg, "error").into_response());
    }
    let points = form
        .points
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|_| AppError::BadRequest("Points must be a number".to_string()))?;
    match settings_model::create_scale(
        &state.pool,
        form.name.trim(),
        min,
        max,
        form.grade.trim(),
        points,
    )
    .await
    {
        Ok(_) => Ok(flash_redirect("/settings", "Grading scale added", "ok").into_response()),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/settings", &msg, &kind).into_response())
        }
    }
}

async fn scale_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    match settings_model::delete_scale(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect("/settings", "Grading scale deleted", "ok").into_response()),
        Err(AppError::NotFound) => {
            Ok(flash_redirect("/settings", "Not found", "error").into_response())
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize)]
struct SemesterForm {
    school_year_id: String,
    name: String,
    start_date: String,
    end_date: String,
    is_current: Option<String>,
}

async fn semester_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<SemesterForm>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    let year_id: i64 = match form.school_year_id.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(
                flash_redirect("/settings", "School year is required", "error").into_response(),
            );
        }
    };
    let start = match NaiveDate::parse_from_str(form.start_date.trim(), "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return Ok(
                flash_redirect("/settings", "Start date must be YYYY-MM-DD", "error")
                    .into_response(),
            );
        }
    };
    let end = match NaiveDate::parse_from_str(form.end_date.trim(), "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return Ok(
                flash_redirect("/settings", "End date must be YYYY-MM-DD", "error").into_response(),
            );
        }
    };
    if let Err(msg) = settings_model::validate_semester(&form.name, start, end) {
        return Ok(flash_redirect("/settings", &msg, "error").into_response());
    }
    // Semester dates must fall within the selected school year.
    if let Ok(year) = class_model::year_row(&state.pool, year_id).await {
        if start < year.start_date || end > year.end_date {
            return Ok(flash_redirect(
                "/settings",
                "Semester dates must fall within the school year",
                "error",
            )
            .into_response());
        }
    }
    match settings_model::create_semester(&state.pool, year_id, form.name.trim(), start, end).await
    {
        Ok(row) => {
            if form.is_current.as_deref() == Some("on")
                || form.is_current.as_deref() == Some("true")
            {
                // Propagate a set-current failure instead of silently swallowing it.
                if let Err(e) = settings_model::set_current_semester(&state.pool, row.id).await {
                    let (msg, kind) = db_flash(e);
                    return Ok(flash_redirect("/settings", &msg, &kind).into_response());
                }
            }
            Ok(flash_redirect("/settings", "Semester created", "ok").into_response())
        }
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/settings", &msg, &kind).into_response())
        }
    }
}

async fn semester_set_current(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    match settings_model::set_current_semester(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect("/settings", "Current semester updated", "ok").into_response()),
        Err(AppError::NotFound) => {
            Ok(flash_redirect("/settings", "Not found", "error").into_response())
        }
        Err(e) => Err(e),
    }
}

async fn semester_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    match settings_model::delete_semester(&state.pool, id).await {
        Ok(()) => Ok(flash_redirect("/settings", "Semester deleted", "ok").into_response()),
        Err(AppError::NotFound) => {
            Ok(flash_redirect("/settings", "Not found", "error").into_response())
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize)]
struct RawForm {
    key: String,
    value: String,
}

async fn raw_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RawForm>,
) -> Result<Response, AppError> {
    check_admin(&user)?;
    if let Err(msg) = settings_model::validate_setting(&form.key) {
        return Ok(flash_redirect("/settings", &msg, "error").into_response());
    }
    match settings_model::set_setting(&state.pool, form.key.trim(), form.value.trim()).await {
        Ok(_) => Ok(flash_redirect("/settings", "Setting saved", "ok").into_response()),
        Err(e) => {
            let (msg, kind) = db_flash(e);
            Ok(flash_redirect("/settings", &msg, &kind).into_response())
        }
    }
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
    fn flash_redirect_works() {
        let (status, loc) = location(flash_redirect("/settings", "ok", "ok"));
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(loc, "/settings?flash=ok&flash_kind=ok");
    }
}
