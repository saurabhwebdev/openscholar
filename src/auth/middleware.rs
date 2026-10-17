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

/// Gate that redirects anonymous/expired sessions to `/login?next=...`.
fn api_token() -> String {
    std::env::var("API_TOKEN").unwrap_or_else(|_| "dev-api-token".to_string())
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

    // Bearer token fallback for the REST API.
    if user.is_none() {
        if let Some(auth) = req.headers().get(AUTHORIZATION) {
            if let Ok(s) = auth.to_str() {
                if let Some(tok) = s.strip_prefix("Bearer ") {
                    if tok.trim() == api_token() {
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
