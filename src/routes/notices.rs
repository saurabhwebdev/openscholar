use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::bulletin as model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notices", get(list_bulletin).post(create_notice_submit))
        .route("/notices/{id}/delete", post(delete_notice_submit))
        .route("/events", post(create_event_submit))
        .route("/events/{id}/delete", post(delete_event_submit))
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

fn can_manage(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Staff])
}

fn check_manage(user: &CurrentUser) -> Result<(), AppError> {
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Staff])
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

// ---------------------------------------------------------------------------
// GET /notices (every authenticated role; notices filtered by role)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "notices/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    notices: Vec<model::NoticeView>,
    events: Vec<model::EventView>,
    can_manage: bool,
}

async fn list_bulletin(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    let notice_rows = model::visible_notices(&state.pool, &user.role).await?;
    let event_rows = model::upcoming_events(&state.pool, 30).await?;
    let now = chrono::Utc::now();
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "bulletin",
        section_label: "Bulletin",
        today: today_display(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        notices: notice_rows
            .iter()
            .map(|r| model::notice_to_view(r, now))
            .collect(),
        events: event_rows.iter().map(model::event_to_view).collect(),
        can_manage: can_manage(&user),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /notices (admin + staff)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct NoticeForm {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    expires_at: String,
}

async fn create_notice_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<NoticeForm>,
) -> Result<impl IntoResponse, AppError> {
    check_manage(&user)?;
    let audience = match model::validate_notice(&form.title, &form.body, &form.audience) {
        Ok(a) => a,
        Err(msg) => return Ok(flash_redirect("/notices", &msg, "error")),
    };
    let expires_at = match model::parse_optional_moment(&form.expires_at, "Expiry") {
        Ok(v) => v,
        Err(msg) => return Ok(flash_redirect("/notices", &msg, "error")),
    };
    match model::create_notice(
        &state.pool,
        form.title.trim(),
        form.body.trim(),
        &audience,
        expires_at,
        Some(user.id),
    )
    .await
    {
        Ok(_) => Ok(flash_redirect("/notices", "Notice published", "ok")),
        Err(e) => {
            tracing::error!(error = %e, "create notice failed");
            Ok(flash_redirect(
                "/notices",
                "Could not publish notice",
                "error",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// POST /notices/:id/delete (admin + staff)
// ---------------------------------------------------------------------------

async fn delete_notice_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_manage(&user)?;
    if model::delete_notice(&state.pool, id).await? {
        Ok(flash_redirect("/notices", "Notice deleted", "ok"))
    } else {
        Ok(flash_redirect("/notices", "Notice not found", "error"))
    }
}

// ---------------------------------------------------------------------------
// POST /events (admin + staff)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EventForm {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    starts_at: String,
    #[serde(default)]
    ends_at: String,
    #[serde(default)]
    location: String,
}

async fn create_event_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<EventForm>,
) -> Result<impl IntoResponse, AppError> {
    check_manage(&user)?;
    let starts_at = match model::parse_moment(&form.starts_at, "Start") {
        Ok(v) => v,
        Err(msg) => return Ok(flash_redirect("/notices", &msg, "error")),
    };
    let ends_at = match model::parse_optional_moment(&form.ends_at, "End") {
        Ok(v) => v,
        Err(msg) => return Ok(flash_redirect("/notices", &msg, "error")),
    };
    if let Err(msg) = model::validate_event(&form.title, starts_at, ends_at) {
        return Ok(flash_redirect("/notices", &msg, "error"));
    }
    match model::create_event(
        &state.pool,
        form.title.trim(),
        &form.description,
        starts_at,
        ends_at,
        &form.location,
        Some(user.id),
    )
    .await
    {
        Ok(_) => Ok(flash_redirect("/notices", "Event added", "ok")),
        Err(e) => {
            tracing::error!(error = %e, "create event failed");
            Ok(flash_redirect("/notices", "Could not add event", "error"))
        }
    }
}

// ---------------------------------------------------------------------------
// POST /events/:id/delete (admin + staff)
// ---------------------------------------------------------------------------

async fn delete_event_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_manage(&user)?;
    if model::delete_event(&state.pool, id).await? {
        Ok(flash_redirect("/notices", "Event deleted", "ok"))
    } else {
        Ok(flash_redirect("/notices", "Event not found", "error"))
    }
}
