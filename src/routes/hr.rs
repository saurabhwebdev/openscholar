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
use crate::models::staff::{list_staff, StaffView};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/hr", get(hr_list))
        .route("/hr/my", get(hr_my))
        .route("/hr/attendance", post(hr_attendance))
        .route("/hr/leave", post(hr_leave))
        .route("/hr/leave/{id}/approve", post(hr_leave_approve))
        .route("/hr/leave/{id}/reject", post(hr_leave_reject))
        .route("/hr/payroll", post(hr_payroll))
        .route("/hr/payroll/{id}/delete", post(hr_payroll_delete))
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

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
}

fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
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

fn ok_redirect(form_path: &str, msg: &str) -> Response {
    Redirect::to(&format!(
        "{form_path}?flash={}&flash_kind=ok",
        util::url_encode(msg)
    ))
    .into_response()
}

fn validate_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| "Date must be YYYY-MM-DD".to_string())
}

fn parse_money_cents(raw: &str) -> Result<i64, String> {
    let cleaned: String = raw.trim().replace([',', ' ', '_'], "");
    if cleaned.is_empty() {
        return Ok(0);
    }
    if cleaned.starts_with('-') {
        return Err("Amount must be non-negative".to_string());
    }
    let (major, minor) = match cleaned.split_once('.') {
        Some((m, f)) => (m, Some(f)),
        None => (cleaned.as_str(), None),
    };
    if major.is_empty() {
        return Err("Amount is not a number".to_string());
    }
    let major_val: i64 = major
        .parse()
        .map_err(|_| "Amount is not a number".to_string())?;
    let minor_val: i64 = match minor {
        None => 0,
        Some(f) => match f.len() {
            0 => 0,
            1 => f
                .parse::<i64>()
                .map_err(|_| "Amount is not a number".to_string())?
                * 10,
            2 => f
                .parse::<i64>()
                .map_err(|_| "Amount is not a number".to_string())?,
            _ => return Err("Amount has too many decimal places".to_string()),
        },
    };
    if major_val < 0 || minor_val < 0 {
        return Err("Amount must be non-negative".to_string());
    }
    Ok(major_val * 100 + minor_val)
}

// Views
struct AttendanceView {
    id: i64,
    date: String,
    status: String,
    note: String,
}

struct LeaveView {
    id: i64,
    staff_name: String,
    leave_type: String,
    start_date: String,
    end_date: String,
    reason: String,
    status: String,
}

struct PayrollView {
    id: i64,
    staff_name: String,
    period_label: String,
    base: String,
    allowance: String,
    deduction: String,
    net: String,
    paid_on: String,
}

#[derive(Template)]
#[template(path = "hr/list.html")]
struct HrListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    staff: Vec<StaffView>,
    selected_staff_id: Option<i64>,
    month: String,
    attendance: Vec<AttendanceView>,
    leaves: Vec<LeaveView>,
    payrolls: Vec<PayrollView>,
    summary_present: i64,
    summary_absent: i64,
    summary_leave: i64,
    summary_late: i64,
    can_mutate: bool,
}

async fn hr_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let (user_name, user_initial) = user_ctx(&user);
    let (flash, flash_kind) = flash_ctx(&params);
    let month = params.get("month").cloned().filter(|s| !s.trim().is_empty()).unwrap_or_else(current_month);
    let selected_staff_id = params
        .get("staff_id")
        .and_then(|s| s.trim().parse::<i64>().ok());

    let staff_rows = list_staff(&state.pool).await?;
    let staff_views: Vec<StaffView> = staff_rows.iter().cloned().map(StaffView::from).collect();
    let name_map: HashMap<i64, String> = staff_rows.iter().map(|s| (s.id, format!("{} {}", s.first_name, s.last_name))).collect();

    let attendance = if let Some(sid) = selected_staff_id {
        hr_model::list_attendance(&state.pool, sid, month.as_str())
            .await?
            .into_iter()
            .map(|r| AttendanceView {
                id: r.id,
                date: r.date.format("%Y-%m-%d").to_string(),
                status: r.status,
                note: r.note.unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let leaves_rows = hr_model::list_leaves(&state.pool).await?;
    let leaves: Vec<LeaveView> = leaves_rows
        .into_iter()
        .map(|r| LeaveView {
            id: r.id,
            staff_name: name_map.get(&r.staff_id).cloned().unwrap_or_else(|| format!("#{}", r.staff_id)),
            leave_type: r.leave_type,
            start_date: r.start_date.format("%Y-%m-%d").to_string(),
            end_date: r.end_date.format("%Y-%m-%d").to_string(),
            reason: r.reason.unwrap_or_default(),
            status: r.status,
        })
        .collect();

    let payroll_rows = hr_model::list_payroll(&state.pool).await?;
    let payrolls: Vec<PayrollView> = payroll_rows
        .into_iter()
        .map(|r| PayrollView {
            id: r.id,
            staff_name: name_map.get(&r.staff_id).cloned().unwrap_or_else(|| format!("#{}", r.staff_id)),
            period_label: r.period_label,
            base: util::format_cents(r.base_cents),
            allowance: util::format_cents(r.allowance_cents),
            deduction: util::format_cents(r.deduction_cents),
            net: util::format_cents(r.net_cents),
            paid_on: r.paid_on.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default(),
        })
        .collect();

    let (summary_present, summary_absent, summary_leave, summary_late) = if let Some(sid) = selected_staff_id {
        match hr_model::monthly_summary_for_staff(&state.pool, sid, month.as_str()).await {
            Ok(s) => (s.present, s.absent, s.leave, s.late),
            Err(_) => (0, 0, 0, 0),
        }
    } else {
        (0, 0, 0, 0)
    };

    let can_mutate = user.has_role(&[Role::Admin, Role::Staff]);

    let template = HrListTemplate {
        section: "hr",
        section_label: "HR",
        today: today(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        staff: staff_views,
        selected_staff_id,
        month,
        attendance,
        leaves,
        payrolls,
        summary_present,
        summary_absent,
        summary_leave,
        summary_late,
        can_mutate,
    };
    template.render().map(Html).map_err(AppError::from)
}

// Forms
#[derive(Debug, Deserialize)]
struct AttendanceForm {
    #[serde(default)]
    staff_id: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    note: String,
}

async fn hr_attendance(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<AttendanceForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let staff_id: i64 = match form.staff_id.trim().parse() {
        Ok(id) => id,
        Err(_) => return Ok(fail_redirect("/hr", "Choose a staff member")),
    };
    let date = match validate_date(&form.date) {
        Ok(d) => d,
        Err(msg) => {
            let m = current_month();
            return Ok(fail_redirect(&format!("/hr?staff_id={staff_id}&month={m}"), &msg));
        }
    };
    let month = date.format("%Y-%m").to_string();
    let note = if form.note.trim().is_empty() { None } else { Some(form.note.as_str()) };
    match hr_model::upsert_attendance(&state.pool, staff_id, date, form.status.trim(), note, Some(user.id)).await {
        Ok(_) => Ok(ok_redirect(&format!("/hr?staff_id={staff_id}&month={month}"), "Attendance saved")),
        Err(AppError::BadRequest(msg)) => Ok(fail_redirect(&format!("/hr?staff_id={staff_id}&month={month}"), &msg)),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize)]
struct LeaveForm {
    #[serde(default)]
    staff_id: String,
    #[serde(default)]
    leave_type: String,
    #[serde(default)]
    start_date: String,
    #[serde(default)]
    end_date: String,
    #[serde(default)]
    reason: String,
}

async fn hr_leave(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<LeaveForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let staff_id: i64 = match form.staff_id.trim().parse() {
        Ok(id) => id,
        Err(_) => return Ok(fail_redirect("/hr", "Choose a staff member")),
    };
    let start = match validate_date(&form.start_date) {
        Ok(d) => d,
        Err(msg) => return Ok(fail_redirect("/hr", &msg)),
    };
    let end = match validate_date(&form.end_date) {
        Ok(d) => d,
        Err(msg) => return Ok(fail_redirect("/hr", &msg)),
    };
    let reason = if form.reason.trim().is_empty() { None } else { Some(form.reason.as_str()) };
    match hr_model::create_leave(&state.pool, staff_id, form.leave_type.trim(), start, end, reason).await {
        Ok(_) => Ok(ok_redirect("/hr", "Leave request created")),
        Err(AppError::BadRequest(msg)) => Ok(fail_redirect("/hr", &msg)),
        Err(e) => Err(e),
    }
}

async fn hr_leave_approve(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?; // payroll/leave decisions are admin-only
    match hr_model::approve_leave(&state.pool, id, user.id).await {
        Ok(_) => Ok(ok_redirect("/hr", "Leave approved")),
        Err(AppError::NotFound) => Ok(fail_redirect("/hr", "Leave request not found")),
        Err(AppError::BadRequest(msg)) => Ok(fail_redirect("/hr", &msg)),
        Err(e) => Err(e),
    }
}

async fn hr_leave_reject(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?; // payroll/leave decisions are admin-only
    match hr_model::reject_leave(&state.pool, id, user.id).await {
        Ok(_) => Ok(ok_redirect("/hr", "Leave rejected")),
        Err(AppError::NotFound) => Ok(fail_redirect("/hr", "Leave request not found")),
        Err(AppError::BadRequest(msg)) => Ok(fail_redirect("/hr", &msg)),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize)]
struct PayrollForm {
    #[serde(default)]
    staff_id: String,
    #[serde(default)]
    period_label: String,
    #[serde(default)]
    base: String,
    #[serde(default)]
    allowance: String,
    #[serde(default)]
    deduction: String,
}

async fn hr_payroll(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<PayrollForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?; // payroll/leave decisions are admin-only
    let staff_id: i64 = match form.staff_id.trim().parse() {
        Ok(id) => id,
        Err(_) => return Ok(fail_redirect("/hr", "Choose a staff member")),
    };
    let base = match parse_money_cents(&form.base) {
        Ok(v) => v,
        Err(msg) => return Ok(fail_redirect("/hr", &msg)),
    };
    let allowance = match parse_money_cents(&form.allowance) {
        Ok(v) => v,
        Err(msg) => return Ok(fail_redirect("/hr", &msg)),
    };
    let deduction = match parse_money_cents(&form.deduction) {
        Ok(v) => v,
        Err(msg) => return Ok(fail_redirect("/hr", &msg)),
    };
    match hr_model::create_payroll(&state.pool, staff_id, form.period_label.trim(), base, allowance, deduction).await {
        Ok(_) => Ok(ok_redirect("/hr", "Payroll recorded")),
        Err(AppError::BadRequest(msg)) => Ok(fail_redirect("/hr", &msg)),
        Err(e) => Err(e),
    }
}

async fn hr_payroll_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?; // payroll/leave decisions are admin-only
    match hr_model::delete_payroll(&state.pool, id).await {
        Ok(_) => Ok(ok_redirect("/hr", "Payroll deleted")),
        Err(AppError::NotFound) => Ok(fail_redirect("/hr", "Payroll not found")),
        Err(e) => Err(e),
    }
}

// GET /hr/my
#[derive(Template)]
#[template(path = "hr/my.html")]
struct HrMyTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    staff_name: String,
    month: String,
    attendance: Vec<AttendanceView>,
    leaves: Vec<LeaveView>,
    payrolls: Vec<PayrollView>,
    summary_present: i64,
    summary_absent: i64,
    summary_leave: i64,
    summary_late: i64,
    has_staff: bool,
}

async fn hr_my(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    let (user_name, user_initial) = user_ctx(&user);
    let month = current_month();
    let staff_row = hr_model::find_staff_row_for_user(&state.pool, &user.email).await?;
    let (staff_name, has_staff, staff_id) = match staff_row {
        Some(r) => (format!("{} {}", r.first_name, r.last_name), true, Some(r.id)),
        None => (user.full_name.clone(), false, None),
    };

    let mut attendance = Vec::new();
    let mut leaves = Vec::new();
    let mut payrolls = Vec::new();
    let (mut sp, mut sa, mut slv, mut slt) = (0i64, 0i64, 0i64, 0i64);

    if let Some(sid) = staff_id {
        attendance = hr_model::list_attendance(&state.pool, sid, month.as_str())
            .await?
            .into_iter()
            .map(|r| AttendanceView {
                id: r.id,
                date: r.date.format("%Y-%m-%d").to_string(),
                status: r.status,
                note: r.note.unwrap_or_default(),
            })
            .collect();
        let lr = hr_model::list_leaves_for_staff(&state.pool, sid).await?;
        leaves = lr
            .into_iter()
            .map(|r| LeaveView {
                id: r.id,
                staff_name: staff_name.clone(),
                leave_type: r.leave_type,
                start_date: r.start_date.format("%Y-%m-%d").to_string(),
                end_date: r.end_date.format("%Y-%m-%d").to_string(),
                reason: r.reason.unwrap_or_default(),
                status: r.status,
            })
            .collect();
        let pr = hr_model::list_payroll_for_staff(&state.pool, sid).await?;
        payrolls = pr
            .into_iter()
            .map(|r| PayrollView {
                id: r.id,
                staff_name: staff_name.clone(),
                period_label: r.period_label,
                base: util::format_cents(r.base_cents),
                allowance: util::format_cents(r.allowance_cents),
                deduction: util::format_cents(r.deduction_cents),
                net: util::format_cents(r.net_cents),
                paid_on: r.paid_on.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default(),
            })
            .collect();
        if let Ok(s) = hr_model::monthly_summary_for_staff(&state.pool, sid, month.as_str()).await {
            sp = s.present;
            sa = s.absent;
            slv = s.leave;
            slt = s.late;
        }
    }

    let template = HrMyTemplate {
        section: "hr",
        section_label: "HR",
        today: today(),
        user_name,
        user_initial,
        staff_name,
        month,
        attendance,
        leaves,
        payrolls,
        summary_present: sp,
        summary_absent: sa,
        summary_leave: slv,
        summary_late: slt,
        has_staff: has_staff,
    };
    template.render().map(Html).map_err(AppError::from)
}
