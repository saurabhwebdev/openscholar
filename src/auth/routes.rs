use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Form, Query, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue,
    },
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::auth::{password, session};
use crate::error::AppError;
use crate::models::user::{find_user_by_email, CurrentUser};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
}

/// Plain-text role of the current session, used by the sidebar script to
/// show only the links a role may use. Always behind `require_auth`.
pub async fn nav_role(Extension(user): Extension<CurrentUser>) -> String {
    user.role.clone()
}

#[derive(Template)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    flash: Option<String>,
    flash_kind: String,
    next: String,
}

async fn login_page(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let template = LoginTemplate {
        flash: params.get("flash").cloned(),
        flash_kind: params
            .get("flash_kind")
            .cloned()
            .unwrap_or_else(|| "info".to_string()),
        next: sanitize_next(params.get("next").map(String::as_str)),
    };
    template.render().map(axum::response::Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    email: String,
    password: String,
    next: Option<String>,
}

async fn login_submit(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let target = sanitize_next(form.next.as_deref());
    let fail = || {
        let mut url = format!(
            "/login?flash={}&flash_kind=error",
            util::url_encode("Invalid email or password")
        );
        if target != "/" {
            url.push_str(&format!("&next={}", util::url_encode(&target)));
        }
        Redirect::to(&url).into_response()
    };

    let user = find_user_by_email(&state.pool, form.email.trim())
        .await?
        .filter(|u| u.active);
    let user = match user {
        Some(u) => u,
        None => return Ok(fail()),
    };
    if password::verify_password(&form.password, &user.password_hash).is_err() {
        return Ok(fail());
    }

    let session_id = session::create_session(&state.pool, user.id).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&session::set_cookie_header(session_id, state.config.cookie_secure))
            .map_err(|e| AppError::Internal(e.to_string()))?,
    );
    Ok((headers, Redirect::to(&target)).into_response())
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(id) = headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(session::session_id_from_cookie_header)
    {
        session::delete_session(&state.pool, id).await?;
    }
    let mut out = HeaderMap::new();
    out.insert(
        SET_COOKIE,
        HeaderValue::from_str(&session::clear_cookie_header(state.config.cookie_secure))
            .map_err(|e| AppError::Internal(e.to_string()))?,
    );
    Ok((out, Redirect::to("/login")).into_response())
}

/// Only allow local redirect targets so `?next=` can't bounce to another site.
fn sanitize_next(next: Option<&str>) -> String {
    match next {
        // Must be a site-local path: a single leading '/', and no backslash or
        // control chars that a browser would renormalise into a protocol-relative
        // ('//host' / '/\host') or header-splitting target.
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.chars().any(|c| c.is_control()) =>
        {
            p.to_string()
        }
        _ => "/dashboard".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_local_paths() {
        assert_eq!(sanitize_next(Some("/students")), "/students");
        assert_eq!(sanitize_next(Some("/dashboard")), "/dashboard");
    }

    #[test]
    fn rejects_remote_and_empty_targets() {
        assert_eq!(sanitize_next(Some("//evil.com/x")), "/dashboard");
        assert_eq!(sanitize_next(Some("https://evil.com")), "/dashboard");
        assert_eq!(sanitize_next(Some("")), "/dashboard");
        assert_eq!(sanitize_next(None), "/dashboard");
        // Backslash / control-char bypasses must also be rejected.
        assert_eq!(sanitize_next(Some("/\\evil.com")), "/dashboard");
        assert_eq!(sanitize_next(Some("/\tevil.com")), "/dashboard");
        assert_eq!(sanitize_next(Some("/\r\nSet-Cookie: x")), "/dashboard");
    }
}
