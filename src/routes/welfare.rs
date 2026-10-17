use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::student::{find_student, StudentView};
use crate::models::user::{CurrentUser, Role};
use crate::models::welfare as w;
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/students/{id}/welfare", get(detail))
        .route("/students/{id}/incidents", post(create_incident))
        .route("/incidents/{id}/delete", post(delete_incident))
        .route("/students/{id}/medical", post(create_medical))
        .route("/medical/{id}/delete", post(delete_medical))
}

fn user_initial(user: &CurrentUser) -> String {
    user.full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
}

fn flash_params(params: &HashMap<String, String>) -> (Option<String>, String) {
    let flash = params.get("flash").cloned();
    let flash_kind = params
        .get("flash_kind")
        .cloned()
        .unwrap_or_else(|| "info".to_string());
    (flash, flash_kind)
}

fn back_to(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

// ---- views ----

#[derive(Clone)]
struct IncidentView {
    id: i64,
    incident_date: String,
    kind: String,
    category: String,
    description: String,
    action_taken: String,
}

impl From<w::IncidentRow> for IncidentView {
    fn from(r: w::IncidentRow) -> Self {
        Self {
            id: r.id,
            incident_date: r.incident_date.to_string(),
            kind: r.kind,
            category: r.category.unwrap_or_default(),
            description: r.description,
            action_taken: r.action_taken.unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
struct MedicalView {
    id: i64,
    record_type: String,
    title: String,
    details: String,
    recorded_on: String,
}

impl From<w::MedicalRow> for MedicalView {
    fn from(r: w::MedicalRow) -> Self {
        Self {
            id: r.id,
            record_type: r.record_type,
            title: r.title,
            details: r.details.unwrap_or_default(),
            recorded_on: r.recorded_on.to_string(),
        }
    }
}

// ---- template ----

#[derive(Template)]
#[template(path = "welfare/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    student: StudentView,
    incidents: Vec<IncidentView>,
    medicals: Vec<MedicalView>,
    summary_minor: i64,
    summary_major: i64,
    summary_positive: i64,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = find_student(&state.pool, id).await?.ok_or(AppError::NotFound)?;
    let student = StudentView::from(row);
    let incidents = w::list_incidents(&state.pool, id)
        .await?
        .into_iter()
        .map(IncidentView::from)
        .collect();
    let medicals = w::list_medical(&state.pool, id)
        .await?
        .into_iter()
        .map(MedicalView::from)
        .collect();
    let summary = w::incident_summary(&state.pool, id).await?;
    let (flash, flash_kind) = flash_params(&params);
    let template = DetailTemplate {
        section: "students",
        section_label: "Welfare",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        student,
        incidents,
        medicals,
        summary_minor: summary.minor,
        summary_major: summary.major,
        summary_positive: summary.positive,
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- forms ----

#[derive(Debug, Deserialize)]
struct IncidentForm {
    #[serde(default)]
    incident_date: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    action_taken: String,
}

#[derive(Debug, Deserialize)]
struct MedicalForm {
    #[serde(default)]
    record_type: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    recorded_on: String,
}

fn parse_date(s: &str) -> Result<Option<NaiveDate>, AppError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| AppError::BadRequest("Invalid date (expected YYYY-MM-DD)".to_string()))
}

fn empty_to_opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

async fn create_incident(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<IncidentForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    if find_student(&state.pool, id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let base = format!("/students/{id}/welfare");
    let kind = if form.kind.trim().is_empty() {
        "minor".to_string()
    } else {
        form.kind.trim().to_string()
    };
    if let Err(msg) = w::validate_incident(&form.description, &kind) {
        return Ok(back_to(&base, &msg).into_response());
    }
    let incident_date = parse_date(&form.incident_date)?
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    let input = w::IncidentInput {
        student_id: id,
        incident_date,
        kind,
        category: empty_to_opt(&form.category),
        description: form.description.clone(),
        action_taken: empty_to_opt(&form.action_taken),
        recorded_by: Some(user.id),
    };
    match w::create_incident(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to(&format!(
            "/students/{id}/welfare?flash={}&flash_kind=ok",
            util::url_encode("Incident recorded")
        ))
        .into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to(&base, &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_incident(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    // find student_id for redirect before delete
    let row = w::find_incident(&state.pool, id).await?;
    let sid = row.map(|r| r.student_id);
    match w::delete_incident(&state.pool, id).await {
        Ok(_) => {
            let target = sid
                .map(|s| format!("/students/{s}/welfare"))
                .unwrap_or_else(|| "/students".to_string());
            Ok(Redirect::to(&format!(
                "{target}?flash={}&flash_kind=ok",
                util::url_encode("Incident deleted")
            ))
            .into_response())
        }
        Err(AppError::NotFound) => Ok(Redirect::to("/students").into_response()),
        Err(e) => Err(e),
    }
}

async fn create_medical(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<MedicalForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    if find_student(&state.pool, id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let base = format!("/students/{id}/welfare");
    let record_type = if form.record_type.trim().is_empty() {
        "note".to_string()
    } else {
        form.record_type.trim().to_string()
    };
    if let Err(msg) = w::validate_medical(&form.title, &record_type) {
        return Ok(back_to(&base, &msg).into_response());
    }
    let recorded_on = parse_date(&form.recorded_on)?
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    let input = w::MedicalInput {
        student_id: id,
        record_type,
        title: form.title.clone(),
        details: empty_to_opt(&form.details),
        recorded_on,
        recorded_by: Some(user.id),
    };
    match w::create_medical(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to(&format!(
            "/students/{id}/welfare?flash={}&flash_kind=ok",
            util::url_encode("Medical record added")
        ))
        .into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to(&base, &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_medical(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = w::find_medical(&state.pool, id).await?;
    let sid = row.map(|r| r.student_id);
    match w::delete_medical(&state.pool, id).await {
        Ok(_) => {
            let target = sid
                .map(|s| format!("/students/{s}/welfare"))
                .unwrap_or_else(|| "/students".to_string());
            Ok(Redirect::to(&format!(
                "{target}?flash={}&flash_kind=ok",
                util::url_encode("Medical record deleted")
            ))
            .into_response())
        }
        Err(AppError::NotFound) => Ok(Redirect::to("/students").into_response()),
        Err(e) => Err(e),
    }
}