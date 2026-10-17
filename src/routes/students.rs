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
use crate::models::student::{
    create_student, find_student, is_duplicate_admission, list_students, search_students,
    update_student, validate_student, StudentInput, StudentView,
};
use crate::models::library::{open_loans_for_student, LoanView};
use crate::models::transport as transport_model;
use crate::models::welfare as welfare_model;
use crate::models::attachment as attachment_model;
use crate::models::hostel as hostel_model;
use crate::models::activity as activity_model;
use crate::models::lesson as lesson_model;
use crate::models::audit as audit_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/students", get(list).post(create))
        .route("/students/new", get(new_form))
        .route("/students/{id}", get(detail))
        .route("/students/{id}/edit", get(edit_form))
        .route("/students/{id}/update", post(update))
        .route("/students/{id}/delete", post(delete_student))
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

fn back_to_form(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

// ---- list ----

#[derive(Template)]
#[template(path = "students/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    students: Vec<StudentView>,
    q: String,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let q = params.get("q").cloned().unwrap_or_default();
    let rows = if q.trim().is_empty() {
        list_students(&state.pool).await?
    } else {
        search_students(&state.pool, &q).await?
    };
    let (flash, flash_kind) = flash_params(&params);
    let template = ListTemplate {
        section: "students",
        section_label: "Students",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        students: rows.into_iter().map(StudentView::from).collect(),
        q,
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- shared form ----

#[derive(Debug, Deserialize)]
struct StudentForm {
    #[serde(default)]
    admission_no: String,
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
    status: String,
    #[serde(default)]
    enrolled_on: String,
}

fn empty_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
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

fn input_from_form(form: &StudentForm) -> Result<StudentInput, AppError> {
    let status = if form.status.trim().is_empty() {
        "active".to_string()
    } else {
        form.status.trim().to_string()
    };
    if let Err(msg) = validate_student(
        &form.admission_no,
        &form.first_name,
        &form.last_name,
        Some(form.gender.as_str()),
        &status,
    ) {
        return Err(AppError::BadRequest(msg));
    }
    Ok(StudentInput {
        admission_no: form.admission_no.trim().to_string(),
        first_name: form.first_name.trim().to_string(),
        last_name: form.last_name.trim().to_string(),
        date_of_birth: parse_date(&form.date_of_birth)?,
        gender: empty_to_none(&form.gender),
        guardian_name: empty_to_none(&form.guardian_name),
        guardian_phone: empty_to_none(&form.guardian_phone),
        guardian_email: empty_to_none(&form.guardian_email),
        address: empty_to_none(&form.address),
        status,
        enrolled_on: parse_date(&form.enrolled_on)?
            .unwrap_or_else(|| chrono::Local::now().date_naive()),
    })
}

#[derive(Template)]
#[template(path = "students/new.html")]
struct FormTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    editing: bool,
    action: String,
    admission_no: String,
    first_name: String,
    last_name: String,
    date_of_birth: String,
    gender: String,
    guardian_name: String,
    guardian_phone: String,
    guardian_email: String,
    address: String,
    status: String,
    enrolled_on: String,
    flash: Option<String>,
    flash_kind: String,
}

fn empty_form(
    user: &CurrentUser,
    editing: bool,
    action: &str,
    params: &HashMap<String, String>,
) -> FormTemplate {
    let (flash, flash_kind) = flash_params(params);
    FormTemplate {
        section: "students",
        section_label: "Students",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(user),
        editing,
        action: action.to_string(),
        admission_no: String::new(),
        first_name: String::new(),
        last_name: String::new(),
        date_of_birth: String::new(),
        gender: String::new(),
        guardian_name: String::new(),
        guardian_phone: String::new(),
        guardian_email: String::new(),
        address: String::new(),
        status: "active".to_string(),
        enrolled_on: String::new(),
        flash,
        flash_kind,
    }
}

async fn new_form(
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let template = empty_form(&user, false, "/students", &params);
    template.render().map(Html).map_err(AppError::from)
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<StudentForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let input = match input_from_form(&form) {
        Ok(input) => input,
        Err(AppError::BadRequest(msg)) => return Ok(back_to_form("/students/new", &msg).into_response()),
        Err(e) => return Err(e),
    };
    match create_student(&state.pool, &input).await {
        Ok(row) => {
            if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "create", "student", Some(row.id), Some("student created")).await {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(Redirect::to(&format!("/students/{}", row.id)).into_response())
        }
        Err(e) if is_duplicate_admission(&e) => Ok(back_to_form(
            "/students/new",
            "That admission number is already taken",
        )
        .into_response()),
        Err(e) => Err(e),
    }
}

// ---- detail ----

#[derive(Debug, sqlx::FromRow)]
struct EnrollmentRow {
    class_name: String,
    enrolled_on: NaiveDate,
}

struct EnrollmentView {
    class_name: String,
    enrolled_on: String,
}

#[derive(Debug, sqlx::FromRow)]
struct LinkedParentView {
    id: i64,
    parent_user_id: i64,
    email: String,
    full_name: String,
    relation: String,
}

#[derive(Debug, sqlx::FromRow)]
struct StudentUserView {
    id: i64,
    email: String,
    full_name: String,
}

#[derive(Template)]
#[template(path = "students/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    student: StudentView,
    enrollments: Vec<EnrollmentView>,
    balance: String,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
    linked_parents: Vec<LinkedParentView>,
    student_user: Option<StudentUserView>,
    open_loans: Vec<LoanView>,
    transport_routes: Vec<transport_model::RouteRow>,
    hostel_allocation: Option<hostel_model::AllocationRow>,
    welfare_minor: i64,
    welfare_major: i64,
    welfare_positive: i64,
    welfare_total: i64,
    attachments: Vec<attachment_model::AttachmentView>,
    lessons: Vec<lesson_model::LessonRow>,
    activities: Vec<activity_model::ActivityRow>,
}

async fn detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let row = find_student(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let enrollments = sqlx::query_as::<_, EnrollmentRow>(
        "SELECT c.name AS class_name, e.enrolled_on AS enrolled_on
         FROM enrollments e JOIN classes c ON c.id = e.class_id
         WHERE e.student_id = $1 ORDER BY c.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    // Postgres SUM(bigint) returns NUMERIC, so cast the difference to BIGINT.
    let balance: i64 = sqlx::query_scalar(
        "SELECT (COALESCE((SELECT SUM(amount_cents) FROM invoices WHERE student_id = $1), 0)
                - COALESCE((SELECT SUM(p.amount_cents) FROM payments p
                            JOIN invoices i ON i.id = p.invoice_id
                            WHERE i.student_id = $1), 0))::BIGINT",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    let (flash, flash_kind) = flash_params(&params);
    let linked_parents = sqlx::query_as::<_, LinkedParentView>(
        "SELECT pl.id AS id, pl.parent_user_id AS parent_user_id,
                u.email AS email, u.full_name AS full_name, pl.relation AS relation
         FROM parent_links pl JOIN users u ON u.id = pl.parent_user_id
         WHERE pl.student_id = $1 ORDER BY u.full_name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let student_user = sqlx::query_as::<_, StudentUserView>(
        "SELECT u.id AS id, u.email AS email, u.full_name AS full_name
         FROM students s JOIN users u ON u.id = s.user_id
         WHERE s.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let open_loans = open_loans_for_student(&state.pool, id).await.unwrap_or_default();
    let transport_routes = transport_model::routes_for_student(&state.pool, id)
        .await
        .unwrap_or_default();
    let hostel_allocation = hostel_model::open_allocation_for_student(&state.pool, id)
        .await
        .unwrap_or(None);
    let wsummary = welfare_model::incident_summary(&state.pool, id).await.unwrap_or_default();
    let attachments = attachment_model::list_for(&state.pool, "student", id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(attachment_model::AttachmentView::from)
        .collect();
    let lessons = lesson_model::list_for_student(&state.pool, id)
        .await
        .unwrap_or_default();
    let activities = activity_model::activities_for_student(&state.pool, id)
        .await
        .unwrap_or_default();
    let template = DetailTemplate {
        section: "students",
        section_label: "Students",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        student: StudentView::from(row),
        enrollments: enrollments
            .into_iter()
            .map(|e| EnrollmentView {
                class_name: e.class_name,
                enrolled_on: e.enrolled_on.to_string(),
            })
            .collect(),
        balance: util::format_cents(balance),
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
        linked_parents,
        student_user,
        open_loans,
        transport_routes,
        hostel_allocation,
        welfare_minor: wsummary.minor,
        welfare_major: wsummary.major,
        welfare_positive: wsummary.positive,
        welfare_total: wsummary.total,
        attachments,
        lessons,
        activities,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- edit / update ----

async fn edit_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = find_student(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let view = StudentView::from(row);
    let (flash, flash_kind) = flash_params(&params);
    let template = FormTemplate {
        section: "students",
        section_label: "Students",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        editing: true,
        action: format!("/students/{id}/update"),
        admission_no: view.admission_no,
        first_name: view.first_name,
        last_name: view.last_name,
        date_of_birth: view.date_of_birth,
        gender: view.gender,
        guardian_name: view.guardian_name,
        guardian_phone: view.guardian_phone,
        guardian_email: view.guardian_email,
        address: view.address,
        status: view.status,
        enrolled_on: view.enrolled_on,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

async fn update(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<StudentForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let edit_url = format!("/students/{id}/edit");
    if find_student(&state.pool, id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let input = match input_from_form(&form) {
        Ok(input) => input,
        Err(AppError::BadRequest(msg)) => {
            return Ok(back_to_form(&edit_url, &msg).into_response());
        }
        Err(e) => return Err(e),
    };
    match update_student(&state.pool, id, &input).await {
        Ok(row) => {
            if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "update", "student", Some(row.id), Some("student updated")).await {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(Redirect::to(&format!("/students/{id}")).into_response())
        }
        Err(e) if is_duplicate_admission(&e) => Ok(back_to_form(
            &edit_url,
            "That admission number is already taken",
        )
        .into_response()),
        Err(e) => Err(e),
    }
}

// ---- delete ----

/// Runtime SQL for the student hard-delete (child rows cascade via FK).
pub const DELETE_SQL: &str = "DELETE FROM students WHERE id = $1";

/// JS-confirm prompt for the danger-zone form; always names the record.
pub fn confirm_message(full_name: &str) -> String {
    format!("Delete {}? This cannot be undone.", full_name.trim())
}

/// List redirect carrying the success flash.
pub fn delete_success_target() -> String {
    format!(
        "/students?flash={}&flash_kind=ok",
        util::url_encode("Student deleted")
    )
}

async fn delete_student(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let done = sqlx::query(DELETE_SQL)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "delete", "student", Some(id), Some("student deleted")).await {
        tracing::warn!(error = %e, "audit log failed");
    }
    Ok(Redirect::to(&delete_success_target()).into_response())
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
        assert!(matches!(
            parse_date("2024-13-40"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn confirm_message_names_the_student() {
        let msg = confirm_message("Ada Lovelace");
        assert!(!msg.is_empty());
        assert!(msg.contains("Ada Lovelace"));
    }

    #[test]
    fn confirm_message_warns_it_is_irreversible() {
        let msg = confirm_message("Ada Lovelace");
        assert!(msg.contains("cannot be undone"));
    }

    #[test]
    fn delete_sql_targets_students_table() {
        assert!(DELETE_SQL.contains("DELETE FROM students"));
    }

    #[test]
    fn delete_success_target_carries_ok_flash() {
        assert_eq!(
            delete_success_target(),
            "/students?flash=Student+deleted&flash_kind=ok"
        );
    }
}
