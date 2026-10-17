use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};

use crate::comms::mailer::{send_one, SmtpConfig};
use crate::error::AppError;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/outbox", get(outbox_page))
        .route("/outbox/send-due", post(send_due_submit))
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

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

/// Pill class for an outbox status (mirrors the DB CHECK values).
fn status_class(status: &str) -> &'static str {
    match status {
        "sent" => "pill-ok",
        "failed" => "pill-err",
        _ => "pill-warn",
    }
}

/// Flash line for the send-due sweep; pure so the format stays stable.
fn send_summary(sent: usize, failed: usize) -> String {
    format!("Sent {sent}, failed {failed}")
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct OutboxCounts {
    queued: i64,
    sent: i64,
    failed: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OutboxRow {
    to_addr: String,
    subject: String,
    status: String,
    attempts: i32,
    last_error: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DueRow {
    id: i64,
    to_addr: String,
    subject: String,
    body: String,
    attempts: i32,
}

async fn outbox_counts(pool: &sqlx::PgPool) -> Result<OutboxCounts, AppError> {
    let counts = sqlx::query_as::<_, OutboxCounts>(
        "SELECT COUNT(*) FILTER (WHERE status = 'queued') AS queued,
                COUNT(*) FILTER (WHERE status = 'sent') AS sent,
                COUNT(*) FILTER (WHERE status = 'failed') AS failed
         FROM email_outbox",
    )
    .fetch_one(pool)
    .await?;
    Ok(counts)
}

async fn outbox_rows(pool: &sqlx::PgPool) -> Result<Vec<OutboxRow>, AppError> {
    let rows = sqlx::query_as::<_, OutboxRow>(
        "SELECT to_addr, subject, status, attempts, last_error,
                created_at, sent_at
         FROM email_outbox
         ORDER BY id DESC
         LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// GET /outbox (admin only)
// ---------------------------------------------------------------------------

struct MailView {
    to_addr: String,
    subject: String,
    status: String,
    status_class: &'static str,
    attempts: i32,
    created: String,
    sent: Option<String>,
    last_error: Option<String>,
}

#[derive(Template)]
#[template(path = "comms/outbox.html")]
struct OutboxTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    queued: i64,
    sent: i64,
    failed: i64,
    mails: Vec<MailView>,
}

async fn outbox_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(&user, &[Role::Admin])?;

    let counts = outbox_counts(&state.pool).await?;
    let rows = outbox_rows(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = OutboxTemplate {
        section: "outbox",
        section_label: "Email outbox",
        today: today_display(),
        user_name,
        user_initial,
        flash: params.get("flash").cloned(),
        flash_kind: params
            .get("flash_kind")
            .cloned()
            .unwrap_or_else(|| "info".to_string()),
        queued: counts.queued,
        sent: counts.sent,
        failed: counts.failed,
        mails: rows
            .into_iter()
            .map(|r| MailView {
                to_addr: r.to_addr,
                subject: r.subject,
                status: r.status.clone(),
                status_class: status_class(&r.status),
                attempts: r.attempts,
                created: r.created_at.format("%-d %b %Y %H:%M").to_string(),
                sent: r.sent_at.map(|s| s.format("%-d %b %Y %H:%M").to_string()),
                last_error: r.last_error,
            })
            .collect(),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /outbox/send-due (admin only)
// ---------------------------------------------------------------------------

async fn send_due_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(&user, &[Role::Admin])?;

    let due = sqlx::query_as::<_, DueRow>(
        "SELECT id, to_addr, subject, body, attempts
         FROM email_outbox
         WHERE status = 'queued'
         ORDER BY id
         LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;

    let config = SmtpConfig::from_env();
    let mut sent = 0_usize;
    let mut failed = 0_usize;
    for row in due {
        // Blocking SMTP belongs on the blocking pool, never the async worker.
        let cfg = config.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            send_one(&cfg, &row.to_addr, &row.subject, &row.body)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
        match outcome {
            Ok(()) => {
                sent += 1;
                sqlx::query(
                    "UPDATE email_outbox
                     SET status = 'sent', attempts = $1,
                         sent_at = now(), last_error = NULL
                     WHERE id = $2",
                )
                .bind(row.attempts + 1)
                .bind(row.id)
                .execute(&state.pool)
                .await?;
            }
            Err(e) => {
                failed += 1;
                sqlx::query(
                    "UPDATE email_outbox
                     SET status = 'failed', attempts = $1, last_error = $2
                     WHERE id = $3",
                )
                .bind(row.attempts + 1)
                .bind(e.to_string())
                .bind(row.id)
                .execute(&state.pool)
                .await?;
            }
        }
    }

    let msg = send_summary(sent, failed);
    let kind = if failed == 0 { "ok" } else { "error" };
    Ok(flash_redirect("/outbox", &msg, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_statuses_map_to_pills() {
        assert_eq!(status_class("queued"), "pill-warn");
        assert_eq!(status_class("sent"), "pill-ok");
        assert_eq!(status_class("failed"), "pill-err");
        assert_eq!(status_class("bogus"), "pill-warn");
    }

    #[test]
    fn send_summary_counts_both_outcomes() {
        assert_eq!(send_summary(3, 0), "Sent 3, failed 0");
        assert_eq!(send_summary(0, 0), "Sent 0, failed 0");
        assert_eq!(send_summary(2, 1), "Sent 2, failed 1");
    }
}
