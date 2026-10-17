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
use crate::models::hr as hr_model;
use crate::models::staff::{
    classes_for_teacher, create_staff, find_linked_user, find_staff, list_staff, search_staff,
    update_staff, validate_staff, StaffInput, StaffView,
};
use crate::models::audit as audit_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/staff", get(staff_list).post(staff_create))
        .route("/staff/new", get(staff_new))
        .route("/staff/{id}", get(staff_detail))
        .route("/staff/{id}/edit", get(staff_edit))
        .route("/staff/{id}/update", post(staff_update))
        .route("/staff/{id}/delete", post(staff_delete))
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
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

fn flash_ctx(params: &HashMap<String, String>) -> (Option<String>, String) {
    (
        params.get("flash").cloned(),
        params
            .get("flash_kind")
            .cloned()
            .unwrap_or_else(|| "info".to_string()),
    )
}

fn fail_redirect(form_path: &str, msg: &str) -> Response {
    Redirect::to(&format!(
        "{form_path}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
    .into_response()
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// Parse `<input type="date">` (`%Y-%m-%d`); empty means NULL.
fn parse_hired_on(raw: &str) -> Result<Option<NaiveDate>, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| AppError::BadRequest("Hired date must be YYYY-MM-DD".to_string()))
}

fn empty_to_none(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct StaffForm {
    #[serde(default)]
    employee_no: String,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    job_title: String,
    #[serde(default)]
    department: String,
    #[serde(default)]
    hired_on: String,
    #[serde(default)]
    status: String,
}

fn form_to_input(form: &StaffForm, hired_on: Option<NaiveDate>) -> StaffInput {
    StaffInput {
        employee_no: form.employee_no.trim().to_string(),
        first_name: form.first_name.trim().to_string(),
        last_name: form.last_name.trim().to_string(),
        email: empty_to_none(&form.email),
        phone: empty_to_none(&form.phone),
        job_title: empty_to_none(&form.job_title),
        department: empty_to_none(&form.department),
        hired_on,
        status: form.status.trim().to_string(),
    }
}

// ---------- list ----------

#[derive(Template)]
#[template(path = "staff/list.html")]
struct StaffListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    q: String,
    flash: Option<String>,
    flash_kind: String,
    staff: Vec<StaffView>,
}

async fn staff_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let q = params
        .get("q")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let rows = match &q {
        Some(term) => search_staff(&state.pool, term).await?,
        None => list_staff(&state.pool).await?,
    };
    let (user_name, user_initial) = user_ctx(&user);
    let (flash, flash_kind) = flash_ctx(&params);
    let template = StaffListTemplate {
        section: "staff",
        section_label: "Staff",
        today: today(),
        user_name,
        user_initial,
        q: q.unwrap_or_default(),
        flash,
        flash_kind,
        staff: rows.into_iter().map(StaffView::from).collect(),
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---------- new / create ----------

#[derive(Template)]
#[template(path = "staff/new.html")]
struct StaffFormTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    editing: bool,
    action: String,
    cancel_url: String,
    employee_no: String,
    first_name: String,
    last_name: String,
    email: String,
    phone: String,
    job_title: String,
    department: String,
    hired_on: String,
    status: String,
    flash: Option<String>,
}

fn empty_form(
    user: &CurrentUser,
    flash: Option<String>,
    editing: bool,
    action: String,
    cancel_url: String,
) -> StaffFormTemplate {
    let (user_name, user_initial) = user_ctx(user);
    StaffFormTemplate {
        section: "staff",
        section_label: "Staff",
        today: today(),
        user_name,
        user_initial,
        editing,
        action,
        cancel_url,
        employee_no: String::new(),
        first_name: String::new(),
        last_name: String::new(),
        email: String::new(),
        phone: String::new(),
        job_title: String::new(),
        department: String::new(),
        hired_on: String::new(),
        status: "active".to_string(),
        flash,
    }
}

async fn staff_new(
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let template = empty_form(
        &user,
        params.get("flash").cloned(),
        false,
        "/staff".to_string(),
        "/staff".to_string(),
    );
    template.render().map(Html).map_err(AppError::from)
}

async fn staff_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<StaffForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    if let Err(e) = validate_staff(&form.first_name, &form.last_name, &form.employee_no, &form.status)
    {
        return Ok(fail_redirect("/staff/new", &e.to_string()));
    }
    let hired_on = match parse_hired_on(&form.hired_on) {
        Ok(v) => v,
        Err(e) => return Ok(fail_redirect("/staff/new", &e.to_string())),
    };
    let input = form_to_input(&form, hired_on);
    match create_staff(&state.pool, &input).await {
        Ok(row) => {
            if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "create", "staff", Some(row.id), Some("staff created")).await {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(Redirect::to(&format!("/staff/{}", row.id)).into_response())
        }
        Err(AppError::Db(e)) if is_unique_violation(&e) => Ok(fail_redirect(
            "/staff/new",
            "Employee number already exists",
        )),
        Err(e) => Err(e),
    }
}

// ---------- detail ----------

struct StaffClassView {
    id: i64,
    name: String,
    grade_level: String,
    room: String,
    school_year: String,
}

struct LinkedUserView {
    email: String,
    full_name: String,
    role: String,
}

#[derive(Template)]
#[template(path = "staff/detail.html")]
struct StaffDetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    staff: StaffView,
    classes: Vec<StaffClassView>,
    linked_user: Option<LinkedUserView>,
    can_edit: bool,
    hr_month: String,
    hr_attendance: String,
    hr_leave: String,
    hr_payroll: String,
}

fn dash(value: &Option<String>) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => v.clone(),
        _ => "—".to_string(),
    }
}

async fn staff_detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let row = find_staff(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let class_rows = classes_for_teacher(&state.pool, id).await?;
    let linked_row = match &row.email {
        Some(email) if !email.trim().is_empty() => {
            find_linked_user(&state.pool, email).await?
        }
        _ => None,
    };
    // HR card data
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let att_rows = hr_model::list_attendance(&state.pool, id, month.as_str())
        .await
        .unwrap_or_default();
    let present = att_rows.iter().filter(|r| r.status == "present").count();
    let hr_attendance = format!("{} present of {} recorded this month", present, att_rows.len());
    let leave_rows = hr_model::list_leaves_for_staff(&state.pool, id).await.unwrap_or_default();
    let pending = leave_rows.iter().filter(|r| r.status == "pending").count();
    let approved = leave_rows.iter().filter(|r| r.status == "approved").count();
    let hr_leave = format!("{} pending, {} approved ({} total)", pending, approved, leave_rows.len());
    let payroll_rows = hr_model::list_payroll_for_staff(&state.pool, id).await.unwrap_or_default();
    let hr_payroll = match payroll_rows.first() {
        Some(p) => format!("{} — {}", p.period_label, util::format_cents(p.net_cents)),
        None => "—".to_string(),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let template = StaffDetailTemplate {
        section: "staff",
        section_label: "Staff",
        today: today(),
        user_name,
        user_initial,
        staff: StaffView::from(row),
        classes: class_rows
            .into_iter()
            .map(|c| StaffClassView {
                id: c.id,
                name: c.name,
                grade_level: dash(&c.grade_level),
                room: dash(&c.room),
                school_year: dash(&c.school_year),
            })
            .collect(),
        linked_user: linked_row.map(|u| LinkedUserView {
            email: u.email,
            full_name: u.full_name,
            role: u.role,
        }),
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        hr_month: month,
        hr_attendance,
        hr_leave,
        hr_payroll,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---------- edit / update ----------

async fn staff_edit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = find_staff(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let view = StaffView::from(row);
    let (user_name, user_initial) = user_ctx(&user);
    let template = StaffFormTemplate {
        section: "staff",
        section_label: "Staff",
        today: today(),
        user_name,
        user_initial,
        editing: true,
        action: format!("/staff/{id}/update"),
        cancel_url: format!("/staff/{id}"),
        employee_no: view.employee_no,
        first_name: view.first_name,
        last_name: view.last_name,
        email: view.email.unwrap_or_default(),
        phone: view.phone.unwrap_or_default(),
        job_title: view.job_title.unwrap_or_default(),
        department: view.department.unwrap_or_default(),
        hired_on: view.hired_on_input,
        status: view.status,
        flash: params.get("flash").cloned(),
    };
    template.render().map(Html).map_err(AppError::from)
}

async fn staff_update(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<StaffForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let edit_path = format!("/staff/{id}/edit");
    if let Err(e) = validate_staff(&form.first_name, &form.last_name, &form.employee_no, &form.status)
    {
        return Ok(fail_redirect(&edit_path, &e.to_string()));
    }
    let hired_on = match parse_hired_on(&form.hired_on) {
        Ok(v) => v,
        Err(e) => return Ok(fail_redirect(&edit_path, &e.to_string())),
    };
    let input = form_to_input(&form, hired_on);
    match update_staff(&state.pool, id, &input).await {
        Ok(Some(row)) => {
            if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "update", "staff", Some(row.id), Some("staff updated")).await {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(Redirect::to(&format!("/staff/{}", row.id)).into_response())
        }
        Ok(None) => Err(AppError::NotFound),
        Err(AppError::Db(e)) if is_unique_violation(&e) => Ok(fail_redirect(
            &edit_path,
            "Employee number already exists",
        )),
        Err(e) => Err(e),
    }
}

// ---------- delete ----------

/// Runtime SQL for the staff hard-delete (classes.teacher_id SET NULLs via FK).
pub const DELETE_SQL: &str = "DELETE FROM staff WHERE id = $1";

/// JS-confirm prompt for the danger-zone form; always names the record.
pub fn confirm_message(full_name: &str) -> String {
    format!("Delete {}? This cannot be undone.", full_name.trim())
}

/// List redirect carrying the success flash.
pub fn delete_success_target() -> String {
    format!(
        "/staff?flash={}&flash_kind=ok",
        util::url_encode("Staff deleted")
    )
}

async fn staff_delete(
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
    if let Err(e) = audit_model::log_action(&state.pool, Some(user.id), "delete", "staff", Some(id), Some("staff deleted")).await {
        tracing::warn!(error = %e, "audit log failed");
    }
    Ok(Redirect::to(&delete_success_target()).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_message_names_the_staff_member() {
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
    fn delete_sql_targets_staff_table() {
        assert!(DELETE_SQL.contains("DELETE FROM staff"));
    }

    #[test]
    fn delete_success_target_carries_ok_flash() {
        assert_eq!(
            delete_success_target(),
            "/staff?flash=Staff+deleted&flash_kind=ok"
        );
    }
}
