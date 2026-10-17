use askama::Template;
use axum::{
    Router,
    extract::{Extension, State},
    response::IntoResponse,
    routing::get,
};

use crate::error::AppError;
use crate::models::user::CurrentUser;
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new().route("/dashboard", get(dashboard))
}

#[derive(Debug, sqlx::FromRow)]
struct BalanceRow {
    name: String,
    balance: i64,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    school_year: Option<String>,
    students: i64,
    staff: i64,
    classes: i64,
    outstanding: String,
    balances: Vec<BalanceRowView>,
}

struct BalanceRowView {
    name: String,
    balance: String,
}

async fn dashboard(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    let students: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM students WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await?;
    let staff: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM staff WHERE status = 'active'")
        .fetch_one(&state.pool)
        .await?;
    let classes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM classes
         WHERE school_year_id = (SELECT id FROM school_years WHERE is_current LIMIT 1)",
    )
    .fetch_one(&state.pool)
    .await?;
    let outstanding: i64 = sqlx::query_scalar(
        "SELECT (COALESCE(SUM(i.amount_cents), 0) - COALESCE(SUM(p.paid), 0))::BIGINT
         FROM invoices i
         LEFT JOIN (SELECT invoice_id, SUM(amount_cents) AS paid FROM payments GROUP BY invoice_id) p
           ON p.invoice_id = i.id",
    )
    .fetch_one(&state.pool)
    .await?;
    let school_year: Option<String> =
        sqlx::query_scalar("SELECT name FROM school_years WHERE is_current LIMIT 1")
            .fetch_optional(&state.pool)
            .await?;
    let balances = sqlx::query_as::<_, BalanceRow>(
        "SELECT s.first_name || ' ' || s.last_name AS name,
                (COALESCE(inv.billed, 0) - COALESCE(pay.paid, 0))::BIGINT AS balance
         FROM students s
         LEFT JOIN (SELECT student_id, SUM(amount_cents) AS billed
                    FROM invoices GROUP BY student_id) inv ON inv.student_id = s.id
         LEFT JOIN (SELECT i.student_id, SUM(p.amount_cents) AS paid
                    FROM payments p JOIN invoices i ON i.id = p.invoice_id
                    GROUP BY i.student_id) pay ON pay.student_id = s.id
         WHERE COALESCE(inv.billed, 0) - COALESCE(pay.paid, 0) > 0
         ORDER BY balance DESC
         LIMIT 5",
    )
    .fetch_all(&state.pool)
    .await?;

    let template = DashboardTemplate {
        section: "dashboard",
        section_label: "Dashboard",
        today: chrono::Local::now().format("%A, %-d %B %Y").to_string(),
        user_initial: user
            .full_name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string()),
        user_name: user.full_name.clone(),
        school_year,
        students,
        staff,
        classes,
        outstanding: util::format_cents(outstanding),
        balances: balances
            .into_iter()
            .map(|b| BalanceRowView {
                name: b.name,
                balance: util::format_cents(b.balance),
            })
            .collect(),
    };
    template
        .render()
        .map(axum::response::Html)
        .map_err(AppError::from)
}
