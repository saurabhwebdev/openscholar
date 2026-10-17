use axum::{
    extract::{Request, State},
    http::header::{AUTHORIZATION, COOKIE},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};

use crate::auth::session;
use crate::error::AppError;
use crate::models::user::{find_session_user, CurrentUser, Role};
use crate::state::AppState;
use crate::util;

/// The published dev default token. A deployment that leaves `API_TOKEN` at this
/// value (or unset/empty) gets the REST bearer path DISABLED — fail-closed — so a
/// forgotten config can never ship a publicly-known admin credential.
pub const DEV_API_TOKEN: &str = "dev-api-token";

/// The configured REST API bearer token, or `None` when the bearer path is
/// disabled. Unset, empty/blank, or equal to [`DEV_API_TOKEN`] all disable it.
pub fn configured_api_token() -> Option<String> {
    match std::env::var("API_TOKEN") {
        Ok(t) if !t.trim().is_empty() && t.trim() != DEV_API_TOKEN => Some(t.trim().to_string()),
        _ => None,
    }
}

/// Constant-time byte comparison so the bearer secret can't be recovered via a
/// timing side-channel. Avoids an extra crate dependency.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub async fn require_auth(state: State<AppState>, req: Request, next: Next) -> Response {
    let here = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let is_api = req.uri().path().starts_with("/api/");

    let login_target = format!("/login?next={}", util::url_encode(here));

    let session_id = req
        .headers()
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(session::session_id_from_cookie_header);

    let mut user = match session_id {
        Some(id) => find_session_user(&state.pool, id).await.unwrap_or(None),
        None => None,
    };

    // Bearer-token fallback for the REST API. Restricted to `/api/` routes so a
    // service token can never reach HTML pages carrying student PII (welfare,
    // medical, etc.); those require a real staff session. Disabled entirely
    // unless a non-default `API_TOKEN` is configured (see configured_api_token).
    if user.is_none() && is_api {
        if let (Some(auth), Some(expected)) =
            (req.headers().get(AUTHORIZATION), configured_api_token())
        {
            if let Ok(s) = auth.to_str() {
                if let Some(tok) = s.strip_prefix("Bearer ") {
                    let tok = tok.trim();
                    if !tok.is_empty() && constant_time_eq(tok.as_bytes(), expected.as_bytes()) {
                        user = Some(CurrentUser {
                            id: 0,
                            email: "api@openscholar".to_string(),
                            full_name: "API".to_string(),
                            role: "admin".to_string(),
                        });
                    }
                }
            }
        }
    }

    match user {
        Some(user) => {
            let mut req = req;
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        None => {
            if is_api {
                (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
            } else {
                Redirect::to(&login_target).into_response()
            }
        }
    }
}
/// Role gate for handlers that already run behind [`require_auth`].
pub fn require_roles(user: &CurrentUser, allowed: &[Role]) -> Result<(), AppError> {
    if user.has_role(allowed) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Layer gate for staff-side routes: parents/students are sent to their
/// portal instead of hitting staff pages directly. Runs after `require_auth`.
pub async fn require_staff_side(
    State(_state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let staff_side = req
        .extensions()
        .get::<CurrentUser>()
        .map(|u| u.role_enum().map(|r| r.is_staff_side()).unwrap_or(false))
        .unwrap_or(false);
    if staff_side {
        next.run(req).await
    } else {
        Redirect::to("/portal").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_with_role(role: &str) -> CurrentUser {
        CurrentUser {
            id: 7,
            email: "t@example.com".to_string(),
            full_name: "T".to_string(),
            role: role.to_string(),
        }
    }

    #[test]
    fn allows_listed_roles() {
        let teacher = user_with_role("teacher");
        assert!(require_roles(&teacher, &[Role::Teacher]).is_ok());
        assert!(require_roles(&teacher, &[Role::Admin, Role::Teacher]).is_ok());
    }

    #[test]
    fn denies_unlisted_roles() {
        let teacher = user_with_role("teacher");
        assert!(matches!(
            require_roles(&teacher, &[Role::Admin]),
            Err(AppError::Forbidden)
        ));
    }

    #[test]
    fn staff_side_helper_matches_roles() {
        assert!(user_with_role("admin").role_enum().unwrap().is_staff_side());
        assert!(user_with_role("parent").role_enum().is_ok());
        assert!(!user_with_role("parent").role_enum().unwrap().is_staff_side());
    }
}
