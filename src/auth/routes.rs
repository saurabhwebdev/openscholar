use std::collections::HashMap;
use std::net::SocketAddr;

use askama::Template;
use axum::{
    extract::{ConnectInfo, Extension, Form, Query, State},
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

/// Failed attempts for one account before it is temporarily locked.
const ACCOUNT_MAX_FAILURES: i32 = 5;
/// Window over which failures accumulate, and the lockout length.
const ACCOUNT_WINDOW_MINUTES: i64 = 15;
const ACCOUNT_LOCKOUT_MINUTES: i64 = 15;

/// Seconds a locked account has remaining, or `None` if not locked.
async fn account_lock_remaining(pool: &sqlx::PgPool, email: &str) -> Option<i64> {
    let secs: Option<Option<f64>> = sqlx::query_scalar(
        // Cast to float8: EXTRACT returns `numeric` in modern Postgres, which
        // would fail to decode as f64 and silently disable the lockout.
        "SELECT EXTRACT(EPOCH FROM (locked_until - now()))::float8 FROM login_attempts
         WHERE email_lower = lower($1) AND locked_until IS NOT NULL AND locked_until > now()",
    )
    .bind(email.trim())
    .fetch_optional(pool)
    .await
    .ok()?;
    secs.flatten().map(|s| s.ceil() as i64)
}

/// Record a failed attempt, resetting the window if stale and locking the
/// account once the threshold is reached. Best-effort (errors are ignored).
async fn record_account_failure(pool: &sqlx::PgPool, email: &str) {
    let _ = sqlx::query(
        "INSERT INTO login_attempts (email_lower, fail_count, first_fail_at, locked_until)
         VALUES (lower($1), 1, now(), NULL)
         ON CONFLICT (email_lower) DO UPDATE SET
            fail_count = CASE
                WHEN login_attempts.first_fail_at < now() - ($2 || ' minutes')::interval THEN 1
                ELSE login_attempts.fail_count + 1 END,
            first_fail_at = CASE
                WHEN login_attempts.first_fail_at < now() - ($2 || ' minutes')::interval THEN now()
                ELSE login_attempts.first_fail_at END,
            locked_until = CASE
                WHEN (CASE
                        WHEN login_attempts.first_fail_at < now() - ($2 || ' minutes')::interval THEN 1
                        ELSE login_attempts.fail_count + 1 END) >= $3
                THEN now() + ($4 || ' minutes')::interval
                ELSE login_attempts.locked_until END",
    )
    .bind(email.trim())
    .bind(ACCOUNT_WINDOW_MINUTES.to_string())
    .bind(ACCOUNT_MAX_FAILURES)
    .bind(ACCOUNT_LOCKOUT_MINUTES.to_string())
    .execute(pool)
    .await;
}

async fn clear_account_failures(pool: &sqlx::PgPool, email: &str) {
    let _ = sqlx::query("DELETE FROM login_attempts WHERE email_lower = lower($1)")
        .bind(email.trim())
        .execute(pool)
        .await;
}

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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let target = sanitize_next(form.next.as_deref());
    let ip = peer.ip();
    let redirect = |msg: &str| {
        let mut url = format!("/login?flash={}&flash_kind=error", util::url_encode(msg));
        if target != "/" {
            url.push_str(&format!("&next={}", util::url_encode(&target)));
        }
        Redirect::to(&url).into_response()
    };
    let fail = || redirect("Invalid email or password");
    // Blunt brute-force timing/throughput with a fixed delay on every attempt.
    let slow_fail = |resp: Response| async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        resp
    };

    // Per-IP throttle (in-memory).
    if state.login_limiter.is_blocked(ip) {
        return Ok(slow_fail(redirect(
            "Too many login attempts from this network. Please wait a few minutes and try again.",
        ))
        .await);
    }
    // Per-account lockout (DB-backed).
    if let Some(secs) = account_lock_remaining(&state.pool, &form.email).await {
        let mins = (secs / 60) + 1;
        return Ok(slow_fail(redirect(&format!(
            "This account is temporarily locked after too many failed attempts. Try again in about {mins} minute(s)."
        )))
        .await);
    }

    let user = find_user_by_email(&state.pool, form.email.trim())
        .await?
        .filter(|u| u.active);
    let user = match user {
        Some(u) => u,
        None => {
            state.login_limiter.record_failure(ip);
            record_account_failure(&state.pool, &form.email).await;
            return Ok(slow_fail(fail()).await);
        }
    };
    if password::verify_password(&form.password, &user.password_hash).is_err() {
        state.login_limiter.record_failure(ip);
        record_account_failure(&state.pool, &form.email).await;
        return Ok(slow_fail(fail()).await);
    }

    // Success: clear throttles and start a fresh session.
    state.login_limiter.clear(ip);
    clear_account_failures(&state.pool, &form.email).await;
    let session_id = session::create_session(&state.pool, user.id).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&session::set_cookie_header(session_id, state.config.cookie_secure))
            .map_err(|e| AppError::Internal(e.to_string()))?,
    );
    // Double-submit CSRF token cookie (read by the page script into forms).
    headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&session::csrf_cookie_header(
            &session::new_csrf_token(),
            state.config.cookie_secure,
        ))
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
    out.append(
        SET_COOKIE,
        HeaderValue::from_str(&session::clear_csrf_cookie_header(state.config.cookie_secure))
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
