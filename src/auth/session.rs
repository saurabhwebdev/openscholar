use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

pub const SESSION_COOKIE: &str = "session_id";
pub const SESSION_TTL_DAYS: i64 = 30;

pub fn new_session_id() -> Uuid {
    Uuid::new_v4()
}

pub fn default_expiry() -> DateTime<Utc> {
    Utc::now() + Duration::days(SESSION_TTL_DAYS)
}

pub fn set_cookie_header(session_id: Uuid, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        SESSION_TTL_DAYS * 24 * 3600,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookie_header(secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}=deleted; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

pub const CSRF_COOKIE: &str = "csrf_token";

pub fn new_csrf_token() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Double-submit CSRF cookie. NOT HttpOnly on purpose: the page script reads it
/// to echo the token into form submissions. SameSite=Lax + the value secrecy
/// (an attacker site can't read it cross-origin) are what make it a CSRF token.
pub fn csrf_cookie_header(token: &str, secure: bool) -> String {
    format!(
        "{CSRF_COOKIE}={token}; Path=/; SameSite=Lax; Max-Age={}{}",
        SESSION_TTL_DAYS * 24 * 3600,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_csrf_cookie_header(secure: bool) -> String {
    format!(
        "{CSRF_COOKIE}=deleted; Path=/; SameSite=Lax; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

/// Read a named cookie's value from a Cookie header string.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    for pair in header.split(';') {
        let (k, v) = pair.split_once('=')?;
        if k.trim() == name {
            return Some(v.trim());
        }
    }
    None
}

pub fn session_id_from_cookie_header(header: &str) -> Option<Uuid> {
    for pair in header.split(';') {
        let (name, value) = pair.split_once('=')?;
        if name.trim() == SESSION_COOKIE {
            return value.trim().parse().ok();
        }
    }
    None
}

pub async fn create_session(pool: &PgPool, user_id: i64) -> Result<Uuid, AppError> {
    let id = new_session_id();
    let expires_at = default_expiry();
    sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(user_id)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok(id)
}

pub async fn delete_session(pool: &PgPool, id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Invalidate every session for a user except `keep` (used on self password
/// change so the acting session stays logged in but other devices are kicked).
pub async fn delete_other_sessions(pool: &PgPool, user_id: i64, keep: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND id <> $2")
        .bind(user_id)
        .bind(keep)
        .execute(pool)
        .await?;
    Ok(())
}

/// Invalidate all of a user's sessions (used on admin password reset).
pub async fn delete_all_sessions_for(pool: &PgPool, user_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_unique() {
        assert_ne!(new_session_id(), new_session_id());
    }

    #[test]
    fn expiry_is_about_thirty_days_out() {
        let expiry = default_expiry();
        let now = Utc::now();
        assert!(expiry > now + Duration::days(29));
        assert!(expiry <= now + Duration::days(31));
    }

    #[test]
    fn cookie_headers_round_trip() {
        let id = new_session_id();
        let set = set_cookie_header(id, true);
        assert!(set.contains(&format!("{SESSION_COOKIE}={id}")));
        assert!(set.contains("HttpOnly"));
        assert!(set.contains("SameSite=Lax"));
        assert!(set.contains("Secure"));

        let header = format!("other=1; {set}");
        assert_eq!(session_id_from_cookie_header(&header), Some(id));
    }

    #[test]
    fn secure_flag_is_toggleable() {
        let id = new_session_id();
        assert!(set_cookie_header(id, true).contains("; Secure"));
        assert!(!set_cookie_header(id, false).contains("Secure"));
        assert!(clear_cookie_header(true).contains("; Secure"));
        assert!(!clear_cookie_header(false).contains("Secure"));
    }

    #[test]
    fn cookie_parse_rejects_garbage() {
        assert_eq!(session_id_from_cookie_header("session_id=not-a-uuid"), None);
        assert_eq!(session_id_from_cookie_header("other=1"), None);
        assert_eq!(session_id_from_cookie_header(""), None);
    }

    #[test]
    fn clear_cookie_expires_immediately() {
        let clear = clear_cookie_header(true);
        assert!(clear.contains("Max-Age=0"));
    }
}
