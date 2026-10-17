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

/// Content-Security-Policy. `'unsafe-inline'` is required because the app ships
/// inline `<script>`/`style=` (no nonce infrastructure); Askama auto-escaping is
/// the XSS defense. Still enforces clickjacking (frame-ancestors), base-uri and
/// object/form-action restrictions, and allows only self + Google Fonts.
const CSP: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
font-src 'self' https://fonts.gstatic.com; \
img-src 'self' data:; \
connect-src 'self'; \
frame-ancestors 'none'; base-uri 'self'; object-src 'none'; form-action 'self'";

/// Attach security headers to every response (clickjacking, MIME-sniffing,
/// referrer, transport security). HSTS is ignored by browsers over plain HTTP,
/// so sending it unconditionally is safe.
pub async fn security_headers(req: Request, next: Next) -> Response {
    use axum::http::HeaderValue;
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("content-security-policy", HeaderValue::from_static(CSP));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    );
    resp
}

/// Extract `host[:port]` from an Origin/Referer URL value.
fn url_host(value: &str) -> Option<&str> {
    let after_scheme = value.split("://").nth(1)?;
    Some(after_scheme.split(['/', '?', '#']).next().unwrap_or(after_scheme))
}

/// CSRF defense via Origin/Referer validation: for unsafe methods, if the
/// browser sent an Origin (or Referer), its host must match the request Host.
/// Requests with neither header (non-browser API/CLI clients) are allowed — a
/// browser always sends Origin on a cross-site POST, so this blocks CSRF without
/// per-form tokens or breaking no-JS form submits.
pub async fn csrf_protect(req: Request, next: Next) -> Response {
    use axum::http::Method;
    let unsafe_method = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if !unsafe_method {
        return next.run(req).await;
    }
    let forbidden = || {
        (
            axum::http::StatusCode::FORBIDDEN,
            "Cross-origin request blocked",
        )
            .into_response()
    };
    let headers = req.headers();
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let source = headers
        .get(axum::http::header::ORIGIN)
        .or_else(|| headers.get(axum::http::header::REFERER))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // Primary defense: Origin/Referer must match the request Host.
    if let Some(src) = source {
        return match host.as_deref() {
            Some(h) if url_host(&src) == Some(h) => next.run(req).await,
            _ => forbidden(),
        };
    }

    // No Origin/Referer (API/CLI clients, or a browser that stripped it).
    // Defense-in-depth: if the browser has a CSRF cookie, require the matching
    // double-submit token (header or `_csrf` form field). Clients without the
    // cookie (bearer API) are allowed.
    let cookie_hdr = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok());
    let expected = match cookie_hdr.and_then(|h| crate::auth::session::cookie_value(h, crate::auth::session::CSRF_COOKIE)) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return next.run(req).await,
    };
    // Header token first (htmx/AJAX) — no body buffering needed.
    if let Some(h) = headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) {
        return if constant_time_eq(h.trim().as_bytes(), expected.as_bytes()) {
            next.run(req).await
        } else {
            forbidden()
        };
    }
    // Otherwise read the `_csrf` form field, buffering + restoring the body.
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, 2 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return forbidden(),
    };
    let token_ok = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| {
            s.split('&')
                .find_map(|p| p.split_once('=').filter(|(k, _)| *k == "_csrf").map(|(_, v)| v))
        })
        .map(|t| constant_time_eq(t.trim().as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    let req = Request::from_parts(parts, axum::body::Body::from(bytes));
    if token_ok {
        next.run(req).await
    } else {
        forbidden()
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
