use askama::Template;
use axum::{
    extract::{Extension, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::user::CurrentUser;
use crate::notifications::channels::{ChannelRegistry, InAppChannel};
use crate::notifications::store::{self, NotificationFilter};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(list_notifications))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/clear-read", post(clear_read_notifications))
        .route("/notifications/{id}/read", post(mark_read))
        .route("/notifications/badge", get(badge))
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

/// Only local links become buttons, so a crafted `link` can never bounce off-site.
fn safe_link(raw: Option<&str>) -> Option<String> {
    match raw {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => Some(p.to_string()),
        _ => None,
    }
}

fn kind_class(kind: &str) -> &'static str {
    match kind {
        "ok" | "success" => "pill-ok",
        "warn" | "warning" => "pill-warn",
        "error" => "pill-err",
        _ => "pill-info",
    }
}

/// Build a per-request registry: inbox persistence plus empty extras.
/// Future senders (fees, grades, …) call this then `channels::notify`
/// without any router or state rework.
pub fn registry_for(state: &AppState) -> ChannelRegistry {
    ChannelRegistry::new(InAppChannel::new(state.pool.clone()))
}

// ---------------------------------------------------------------------------
// GET /notifications
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListParams {
    filter: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct NotificationView {
    id: i64,
    kind: String,
    kind_class: &'static str,
    title: String,
    body: Option<String>,
    link: Option<String>,
    is_unread: bool,
    created_at: String,
}

#[derive(Template)]
#[template(path = "notifications/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    active_filter: String,
    unread: i64,
    notifications: Vec<NotificationView>,
}

async fn list_notifications(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    let filter = NotificationFilter::from_param(params.filter.as_deref());
    let rows = store::list_for(&state.pool, user.id, filter).await?;
    let unread = store::unread_count(&state.pool, user.id).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "notifications",
        section_label: "Notifications",
        today: today_display(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        active_filter: filter.as_str().to_string(),
        unread,
        notifications: rows
            .into_iter()
            .map(|r| NotificationView {
                id: r.id,
                kind_class: kind_class(&r.kind),
                kind: r.kind,
                title: r.title,
                body: r.body,
                link: safe_link(r.link.as_deref()),
                is_unread: r.read_at.is_none(),
                created_at: r.created_at.format("%-d %b %Y %H:%M").to_string(),
            })
            .collect(),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /notifications/read-all
// ---------------------------------------------------------------------------

async fn read_all(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    let marked = store::mark_all_read(&state.pool, user.id).await?;
    if marked == 0 {
        Ok(flash_redirect(
            "/notifications",
            "No unread notifications",
            "info",
        ))
    } else {
        Ok(flash_redirect(
            "/notifications",
            "All notifications marked as read",
            "ok",
        ))
    }
}

// ---------------------------------------------------------------------------
// POST /notifications/:id/read
// ---------------------------------------------------------------------------

async fn mark_read(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    if store::mark_read(&state.pool, user.id, id).await? {
        Ok(flash_redirect(
            "/notifications",
            "Notification marked as read",
            "ok",
        ))
    } else {
        Ok(flash_redirect(
            "/notifications",
            "Notification not found",
            "error",
        ))
    }
}

// ---------------------------------------------------------------------------
// POST /notifications/clear-read (any authenticated user; own inbox only)
// ---------------------------------------------------------------------------

async fn clear_read_notifications(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = store::clear_read(&state.pool, user.id).await?;
    let msg = if deleted == 0 {
        "No read notifications to clear".to_string()
    } else if deleted == 1 {
        "Cleared 1 read notification".to_string()
    } else {
        format!("Cleared {deleted} read notifications")
    };
    Ok(flash_redirect("/notifications", &msg, "ok"))
}

// ---------------------------------------------------------------------------
// GET /notifications/badge (HTMX fragment for the sidebar)
// ---------------------------------------------------------------------------

async fn badge(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    let unread = store::unread_count(&state.pool, user.id).await?;
    if unread == 0 {
        Ok(Html(String::new()))
    } else {
        Ok(Html(format!(
            "<span class=\"pill pill-info\">{unread}</span>"
        )))
    }
}
