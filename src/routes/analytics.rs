use askama::Template;
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde_json::json;

use crate::error::AppError;
use crate::models::analytics as am;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/analytics", get(analytics_page))
        .route("/analytics/export", get(analytics_export))
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

fn today_display() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

struct AtRiskView {
    full_name: String,
    admission_no: String,
    fee_balance: String,
    attendance_rate: String,
    avg_grade: String,
    reasons: String,
}

struct TrendView {
    date: String,
    present: i64,
    absent: i64,
    total: i64,
}

struct GradeView {
    class_name: String,
    avg: String,
}

#[derive(Template)]
#[template(path = "analytics/dashboard.html")]
struct AnalyticsTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    at_risk: Vec<AtRiskView>,
    trend: Vec<TrendView>,
    billed: String,
    paid: String,
    outstanding: String,
    grades: Vec<GradeView>,
    students: i64,
    staff: i64,
    classes: i64,
}

async fn analytics_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(&user, &[Role::Admin, Role::Staff])?;
    let at_risk_rows = am::at_risk_students(&state.pool).await?;
    let trend_rows = am::attendance_trend(&state.pool).await?;
    let fee = am::fee_summary(&state.pool).await?;
    let grade_rows = am::grade_summary(&state.pool).await?;
    let counts = am::enrollment_counts(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = AnalyticsTemplate {
        section: "analytics",
        section_label: "Analytics",
        today: today_display(),
        user_name,
        user_initial,
        at_risk: at_risk_rows
            .into_iter()
            .map(|r| AtRiskView {
                full_name: r.full_name,
                admission_no: r.admission_no,
                fee_balance: util::format_cents(r.fee_balance),
                attendance_rate: r
                    .attendance_rate
                    .map(|v| format!("{v:.1}%"))
                    .unwrap_or_else(|| "—".to_string()),
                avg_grade: r
                    .avg_grade
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "—".to_string()),
                reasons: r.reasons,
            })
            .collect(),
        trend: trend_rows
            .into_iter()
            .map(|r| TrendView {
                date: r.date.to_string(),
                present: r.present,
                absent: r.absent,
                total: r.total,
            })
            .collect(),
        billed: util::format_cents(fee.total_billed),
        paid: util::format_cents(fee.total_paid),
        outstanding: util::format_cents(fee.outstanding),
        grades: grade_rows
            .into_iter()
            .map(|g| GradeView {
                class_name: g.class_name,
                avg: if g.has_grades {
                    format!("{:.1}", g.avg)
                } else {
                    "—".to_string()
                },
            })
            .collect(),
        students: counts.students,
        staff: counts.staff,
        classes: counts.classes,
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

async fn analytics_export(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(&user, &[Role::Admin])?;
    let at_risk_rows = am::at_risk_students(&state.pool).await?;
    let fee = am::fee_summary(&state.pool).await?;
    let counts = am::enrollment_counts(&state.pool).await?;
    let body = json!({
        "at_risk": at_risk_rows.iter().map(|r| json!({
            "student_id": r.student_id,
            "full_name": r.full_name,
            "admission_no": r.admission_no,
            "fee_balance": r.fee_balance,
            "attendance_rate": r.attendance_rate,
            "avg_grade": r.avg_grade,
            "reasons": r.reasons,
        })).collect::<Vec<_>>(),
        "fee_summary": {
            "billed": fee.total_billed,
            "paid": fee.total_paid,
            "outstanding": fee.outstanding,
        },
        "enrollment_counts": {
            "students": counts.students,
            "staff": counts.staff,
            "classes": counts.classes,
        }
    });
    Ok((StatusCode::OK, Json(body)))
}