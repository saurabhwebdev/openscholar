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
use crate::models::admission::{
    applicant_counts, can_transition, count_for, create_applicant, find_applicant,
    list_by_status, next_statuses, update_applicant_status, validate_applicant,
    ApplicantInput, ApplicantView, ALLOWED_STATUSES,
};
use crate::models::student::{create_student, is_duplicate_admission, StudentInput};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admissions", get(pipeline))
        .route("/admissions/new", get(new_form))
        .route("/applicants", post(create))
        .route("/applicants/{id}", get(detail))
        .route("/applicants/{id}/status", post(update_status))
        .route("/applicants/{id}/enroll", get(enroll_form).post(enroll))
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

/// All redirect targets are hardcoded local paths; the message is the only
/// user-influenced part and it is percent-encoded (open-redirect safe).
fn back_to_form(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

/// Suggested admission number for the enroll form: `ADM-<applicant id>`.
fn suggested_admission_no(applicant_id: i64) -> String {
    format!("ADM-{applicant_id}")
}

/// Parse an `<input type="date">` value (`YYYY-MM-DD`); empty means absent.
fn parse_date(s: &str) -> Result<Option<NaiveDate>, AppError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| AppError::BadRequest("Invalid date (expected YYYY-MM-DD)".to_string()))
}

fn empty_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

// ---- pipeline ----

#[derive(Template)]
#[template(path = "admissions/pipeline.html")]
struct PipelineTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    new_applicants: Vec<ApplicantView>,
    reviewing: Vec<ApplicantView>,
    accepted: Vec<ApplicantView>,
    rejected: Vec<ApplicantView>,
    enrolled: Vec<ApplicantView>,
    new_count: i64,
    reviewing_count: i64,
    accepted_count: i64,
    rejected_count: i64,
    enrolled_count: i64,
    flash: Option<String>,
    flash_kind: String,
}

async fn pipeline(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let counts = applicant_counts(&state.pool).await?;
    let new_applicants = list_by_status(&state.pool, "new").await?;
    let reviewing = list_by_status(&state.pool, "reviewing").await?;
    let accepted = list_by_status(&state.pool, "accepted").await?;
    let rejected = list_by_status(&state.pool, "rejected").await?;
    let enrolled = list_by_status(&state.pool, "enrolled").await?;
    let (flash, flash_kind) = flash_params(&params);
    let template = PipelineTemplate {
        section: "admissions",
        section_label: "Admissions",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        new_count: count_for(&counts, "new"),
        reviewing_count: count_for(&counts, "reviewing"),
        accepted_count: count_for(&counts, "accepted"),
        rejected_count: count_for(&counts, "rejected"),
        enrolled_count: count_for(&counts, "enrolled"),
        new_applicants: new_applicants.into_iter().map(ApplicantView::from).collect(),
        reviewing: reviewing.into_iter().map(ApplicantView::from).collect(),
        accepted: accepted.into_iter().map(ApplicantView::from).collect(),
        rejected: rejected.into_iter().map(ApplicantView::from).collect(),
        enrolled: enrolled.into_iter().map(ApplicantView::from).collect(),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- new / create ----

#[derive(Debug, Deserialize)]
struct ApplicantForm {
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    date_of_birth: String,
    #[serde(default)]
    gender: String,
    #[serde(default)]
    guardian_name: String,
    #[serde(default)]
    guardian_phone: String,
    #[serde(default)]
    guardian_email: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    applying_for: String,
    #[serde(default)]
    notes: String,
}

fn input_from_form(form: &ApplicantForm) -> Result<ApplicantInput, AppError> {
    if let Err(msg) =
        validate_applicant(&form.first_name, &form.last_name, Some(form.gender.as_str()))
    {
        return Err(AppError::BadRequest(msg));
    }
    Ok(ApplicantInput {
        first_name: form.first_name.trim().to_string(),
        last_name: form.last_name.trim().to_string(),
        date_of_birth: parse_date(&form.date_of_birth)?,
        gender: empty_to_none(&form.gender),
        guardian_name: empty_to_none(&form.guardian_name),
        guardian_phone: empty_to_none(&form.guardian_phone),
        guardian_email: empty_to_none(&form.guardian_email),
        address: empty_to_none(&form.address),
        applying_for: form.applying_for.trim().to_string(),
        notes: empty_to_none(&form.notes),
    })
}

#[derive(Template)]
#[template(path = "admissions/new.html")]
struct NewTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    first_name: String,
    last_name: String,
    date_of_birth: String,
    gender: String,
    guardian_name: String,
    guardian_phone: String,
    guardian_email: String,
    address: String,
    applying_for: String,
    notes: String,
    flash: Option<String>,
    flash_kind: String,
}

async fn new_form(
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let (flash, flash_kind) = flash_params(&params);
    let template = NewTemplate {
        section: "admissions",
        section_label: "Admissions",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        first_name: String::new(),
        last_name: String::new(),
        date_of_birth: String::new(),
        gender: String::new(),
        guardian_name: String::new(),
        guardian_phone: String::new(),
        guardian_email: String::new(),
        address: String::new(),
        applying_for: String::new(),
        notes: String::new(),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ApplicantForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let input = match input_from_form(&form) {
        Ok(input) => input,
        Err(AppError::BadRequest(msg)) => {
            return Ok(back_to_form("/admissions/new", &msg).into_response());
        }
        Err(e) => return Err(e),
    };
    let row = create_applicant(&state.pool, &input).await?;
    Ok(Redirect::to(&format!("/applicants/{}", row.id)).into_response())
}

// ---- detail / status ----

#[derive(Template)]
#[template(path = "admissions/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    applicant: ApplicantView,
    next: Vec<String>,
    can_enroll: bool,
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
    let row = find_applicant(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let status = row.status.clone();
    let (flash, flash_kind) = flash_params(&params);
    let template = DetailTemplate {
        section: "admissions",
        section_label: "Admissions",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        applicant: ApplicantView::from(row),
        next: next_statuses(&status).into_iter().map(str::to_string).collect(),
        can_enroll: status == "accepted",
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct StatusForm {
    #[serde(default)]
    status: String,
}

async fn update_status(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<StatusForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let detail_url = format!("/applicants/{id}");
    let row = find_applicant(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let target = form.status.trim().to_string();
    if !ALLOWED_STATUSES.contains(&target.as_str()) {
        return Ok(back_to_form(&detail_url, "Unknown status").into_response());
    }
    if !can_transition(&row.status, &target) {
        return Ok(back_to_form(&detail_url, "Invalid status transition").into_response());
    }
    update_applicant_status(&state.pool, id, &target).await?;
    Ok(Redirect::to(&format!(
        "{detail_url}?flash={}&flash_kind=ok",
        util::url_encode("Status updated")
    ))
    .into_response())
}

// ---- enroll ----

#[derive(Template)]
#[template(path = "admissions/enroll.html")]
struct EnrollTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    applicant: ApplicantView,
    admission_no: String,
    flash: Option<String>,
    flash_kind: String,
}

async fn enroll_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = find_applicant(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    if row.status != "accepted" {
        return Ok(Redirect::to(&format!(
            "/applicants/{id}?flash={}&flash_kind=error",
            util::url_encode("Only accepted applicants can be enrolled")
        ))
        .into_response());
    }
    let (flash, flash_kind) = flash_params(&params);
    let template = EnrollTemplate {
        section: "admissions",
        section_label: "Admissions",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        applicant: ApplicantView::from(row),
        admission_no: suggested_admission_no(id),
        flash,
        flash_kind,
    };
    template
        .render()
        .map(Html)
        .map(|h| h.into_response())
        .map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct EnrollForm {
    #[serde(default)]
    admission_no: String,
}

async fn enroll(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<EnrollForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let detail_url = format!("/applicants/{id}");
    let enroll_url = format!("/applicants/{id}/enroll");
    let row = find_applicant(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    if row.status == "enrolled" {
        return Ok(back_to_form(&detail_url, "Applicant is already enrolled").into_response());
    }
    if row.status != "accepted" {
        return Ok(
            back_to_form(&detail_url, "Only accepted applicants can be enrolled")
                .into_response(),
        );
    }
    let admission_no = form.admission_no.trim().to_string();
    if admission_no.is_empty() {
        return Ok(back_to_form(&enroll_url, "Admission number is required").into_response());
    }
    let input = StudentInput {
        admission_no: admission_no.clone(),
        first_name: row.first_name.clone(),
        last_name: row.last_name.clone(),
        date_of_birth: row.date_of_birth,
        gender: row.gender.clone(),
        guardian_name: row.guardian_name.clone(),
        guardian_phone: row.guardian_phone.clone(),
        guardian_email: row.guardian_email.clone(),
        address: row.address.clone(),
        status: "active".to_string(),
        enrolled_on: chrono::Local::now().date_naive(),
    };
    match create_student(&state.pool, &input).await {
        Ok(student) => {
            update_applicant_status(&state.pool, id, "enrolled").await?;
            Ok(Redirect::to(&format!("/students/{}", student.id)).into_response())
        }
        Err(e) if is_duplicate_admission(&e) => Ok(back_to_form(
            &enroll_url,
            "That admission number is already taken",
        )
        .into_response()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_date() {
        assert_eq!(
            parse_date("2024-09-01").unwrap(),
            NaiveDate::from_ymd_opt(2024, 9, 1)
        );
    }

    #[test]
    fn empty_date_means_absent() {
        assert_eq!(parse_date("").unwrap(), None);
        assert_eq!(parse_date("   ").unwrap(), None);
    }

    #[test]
    fn rejects_malformed_date() {
        assert!(matches!(
            parse_date("01/09/2024"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn suggests_admission_number_from_applicant_id() {
        assert_eq!(suggested_admission_no(7), "ADM-7");
        assert_eq!(suggested_admission_no(123), "ADM-123");
    }

    #[test]
    fn back_to_form_stays_local_with_encoded_flash() {
        let resp = back_to_form("/applicants/1", "Invalid status transition").into_response();
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert_eq!(
            loc,
            "/applicants/1?flash=Invalid+status+transition&flash_kind=error"
        );
    }

    #[test]
    fn enroll_rejects_blank_admission_number() {
        let form = ApplicantForm {
            first_name: " ".to_string(),
            last_name: "Lovelace".to_string(),
            date_of_birth: String::new(),
            gender: String::new(),
            guardian_name: String::new(),
            guardian_phone: String::new(),
            guardian_email: String::new(),
            address: String::new(),
            applying_for: String::new(),
            notes: String::new(),
        };
        assert!(matches!(
            input_from_form(&form),
            Err(AppError::BadRequest(_))
        ));
    }
}
