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
