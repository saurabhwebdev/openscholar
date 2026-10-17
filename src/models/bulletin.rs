//! Noticeboard + events module (Fedena parity: bulletin + calendar).
//!
//! All SQL here is runtime-checked (`sqlx::query` / `query_as`); no
//! compile-time macros beyond `#[derive(sqlx::FromRow)]`. Timestamps map to
//! `chrono::DateTime<chrono::Utc>`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::AppError;

/// Allowed notice audiences. Family roles see `all` + `parents`;
/// staff-side roles see everything.
pub const AUDIENCES: &[&str] = &["all", "staff", "parents"];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NoticeRow {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub audience: String,
    pub published_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventRow {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Template-friendly views
// ---------------------------------------------------------------------------

pub struct NoticeView {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub audience: String,
    pub audience_class: &'static str,
    pub published: String,
    pub expiry: Option<String>,
}

pub struct EventView {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub starts: String,
    pub ends: Option<String>,
    pub location: Option<String>,
}

pub fn audience_class(audience: &str) -> &'static str {
    match audience {
        "staff" => "pill-warn",
        "parents" => "pill-ok",
        _ => "pill-info",
    }
}

/// Human expiry note: `None` when the notice never expires, otherwise
/// `Expired <date>` / `Expires <date>` against `now`.
pub fn expiry_note(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<String> {
    expires_at.map(|exp| {
        let day = exp.format("%-d %b %Y").to_string();
        if exp < now {
            format!("Expired {day}")
        } else {
            format!("Expires {day}")
        }
    })
}

pub fn notice_to_view(row: &NoticeRow, now: DateTime<Utc>) -> NoticeView {
    NoticeView {
        id: row.id,
        title: row.title.clone(),
        body: row.body.clone(),
        audience: row.audience.clone(),
        audience_class: audience_class(&row.audience),
        published: row.published_at.format("%-d %b %Y %H:%M").to_string(),
        expiry: expiry_note(row.expires_at, now),
    }
}

pub fn event_to_view(row: &EventRow) -> EventView {
    EventView {
        id: row.id,
        title: row.title.clone(),
        description: row.description.clone(),
        starts: row.starts_at.format("%-d %b %Y %H:%M").to_string(),
        ends: row
            .ends_at
            .map(|e| e.format("%-d %b %Y %H:%M").to_string()),
        location: row.location.clone(),
    }
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// `title` / `body` must be non-empty; `audience` must be in [`AUDIENCES`].
/// Returns the trimmed audience for convenience.
pub fn validate_notice(title: &str, body: &str, audience: &str) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("Notice title is required".to_string());
    }
    if body.trim().is_empty() {
        return Err("Notice body is required".to_string());
    }
    let audience = audience.trim();
    if !AUDIENCES.contains(&audience) {
        return Err("Unknown audience".to_string());
    }
    Ok(audience.to_string())
}

/// `title` must be non-empty; when present, `ends_at` must be after `starts_at`.
pub fn validate_event(
    title: &str,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("Event title is required".to_string());
    }
    if let Some(end) = ends_at {
        if end <= starts_at {
            return Err("Event end must be after its start".to_string());
        }
    }
    Ok(())
}

/// Parse a form moment: `datetime-local` (`YYYY-MM-DDTHH:MM`) or a plain
/// date (`YYYY-MM-DD`, taken as midnight UTC).
pub fn parse_moment(raw: &str, field: &str) -> Result<DateTime<Utc>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(format!("{field} is required"));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Ok(naive.and_utc());
    }
    if let Ok(day) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(midnight) = day.and_hms_opt(0, 0, 0) {
            return Ok(midnight.and_utc());
        }
    }
    Err(format!("{field} must be YYYY-MM-DD or YYYY-MM-DDTHH:MM"))
}

/// Optional variant of [`parse_moment`]: blank means "no moment".
pub fn parse_optional_moment(raw: &str, field: &str) -> Result<Option<DateTime<Utc>>, String> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        parse_moment(raw, field).map(Some)
    }
}

/// `true` for staff-side roles (full bulletin visibility).
pub fn is_staff_side_role(role: &str) -> bool {
    matches!(role, "admin" | "teacher" | "staff")
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only)
// ---------------------------------------------------------------------------

/// Notices visible to one role: staff-side roles see everything, family
/// roles (`parent`, `student`) see `all` + `parents`. Newest first.
pub async fn visible_notices(pool: &PgPool, role: &str) -> Result<Vec<NoticeRow>, AppError> {
    if is_staff_side_role(role) {
        return all_notices(pool).await;
    }
    let rows = sqlx::query_as::<_, NoticeRow>(
        "SELECT id, title, body, audience, published_at, expires_at, created_by
         FROM notices
         WHERE audience IN ('all', 'parents')
         ORDER BY published_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every notice, newest first (staff-side management view).
pub async fn all_notices(pool: &PgPool) -> Result<Vec<NoticeRow>, AppError> {
    let rows = sqlx::query_as::<_, NoticeRow>(
        "SELECT id, title, body, audience, published_at, expires_at, created_by
         FROM notices
         ORDER BY published_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_notice(
    pool: &PgPool,
    title: &str,
    body: &str,
    audience: &str,
    expires_at: Option<DateTime<Utc>>,
    created_by: Option<i64>,
) -> Result<NoticeRow, AppError> {
    let row = sqlx::query_as::<_, NoticeRow>(
        "INSERT INTO notices (title, body, audience, expires_at, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, title, body, audience, published_at, expires_at, created_by",
    )
    .bind(title.trim())
    .bind(body.trim())
    .bind(audience.trim())
    .bind(expires_at)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Returns `false` when the id is unknown.
pub async fn update_notice(
    pool: &PgPool,
    id: i64,
    title: &str,
    body: &str,
    audience: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE notices
         SET title = $2, body = $3, audience = $4, expires_at = $5
         WHERE id = $1",
    )
    .bind(id)
    .bind(title.trim())
    .bind(body.trim())
    .bind(audience.trim())
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Returns `false` when the id is unknown.
pub async fn delete_notice(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM notices WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Upcoming calendar strip: events starting within the last day or later,
/// soonest first, capped at `limit` rows.
pub async fn upcoming_events(pool: &PgPool, limit: i64) -> Result<Vec<EventRow>, AppError> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT id, title, description, starts_at, ends_at, location, created_by, created_at
         FROM events
         WHERE starts_at >= now() - interval '1 day'
         ORDER BY starts_at ASC, id ASC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

fn clean_optional(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn create_event(
    pool: &PgPool,
    title: &str,
    description: &str,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    location: &str,
    created_by: Option<i64>,
) -> Result<EventRow, AppError> {
    let row = sqlx::query_as::<_, EventRow>(
        "INSERT INTO events (title, description, starts_at, ends_at, location, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, title, description, starts_at, ends_at, location, created_by, created_at",
    )
    .bind(title.trim())
    .bind(clean_optional(description))
    .bind(starts_at)
    .bind(ends_at)
    .bind(clean_optional(location))
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Returns `false` when the id is unknown.
pub async fn delete_event(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM events WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn moment(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn rejects_empty_notice_title() {
        assert_eq!(
            validate_notice("", "Body", "all"),
            Err("Notice title is required".to_string())
        );
    }

    #[test]
    fn rejects_blank_notice_title() {
        assert!(validate_notice("   ", "Body", "all").is_err());
    }

    #[test]
    fn rejects_empty_notice_body() {
        assert_eq!(
            validate_notice("Title", "  ", "all"),
            Err("Notice body is required".to_string())
        );
    }

    #[test]
    fn rejects_unknown_audience() {
        assert_eq!(
            validate_notice("Title", "Body", "everyone"),
            Err("Unknown audience".to_string())
        );
        assert!(validate_notice("Title", "Body", "").is_err());
    }

    #[test]
    fn accepts_each_audience_and_trims() {
        for audience in AUDIENCES {
            assert_eq!(
                validate_notice("  Title  ", "Body", audience),
                Ok(audience.to_string())
            );
        }
        assert_eq!(
            validate_notice("Title", "Body", "  parents "),
            Ok("parents".to_string())
        );
    }

    #[test]
    fn rejects_empty_event_title() {
        assert_eq!(
            validate_event("", moment(2026, 10, 20, 9, 0), None),
            Err("Event title is required".to_string())
        );
        assert!(validate_event("  ", moment(2026, 10, 20, 9, 0), None).is_err());
    }

    #[test]
    fn rejects_end_before_start() {
        assert_eq!(
            validate_event(
                "Sports day",
                moment(2026, 10, 20, 9, 0),
                Some(moment(2026, 10, 20, 8, 0))
            ),
            Err("Event end must be after its start".to_string())
        );
    }

    #[test]
    fn rejects_end_equal_to_start() {
        let start = moment(2026, 10, 20, 9, 0);
        assert!(validate_event("Sports day", start, Some(start)).is_err());
    }

    #[test]
    fn accepts_missing_end() {
        assert!(validate_event("Assembly", moment(2026, 10, 20, 9, 0), None).is_ok());
    }

    #[test]
    fn accepts_end_after_start() {
        assert!(
            validate_event(
                "Sports day",
                moment(2026, 10, 20, 9, 0),
                Some(moment(2026, 10, 20, 12, 0))
            )
            .is_ok()
        );
    }

    #[test]
    fn parses_datetime_local_moments() {
        assert_eq!(
            parse_moment("2026-10-20T09:30", "Starts at"),
            Ok(moment(2026, 10, 20, 9, 30))
        );
    }

    #[test]
    fn parses_date_only_as_midnight() {
        assert_eq!(
            parse_moment("2026-10-20", "Starts at"),
            Ok(moment(2026, 10, 20, 0, 0))
        );
    }

    #[test]
    fn rejects_blank_and_garbage_moments() {
        assert_eq!(
            parse_moment("", "Starts at"),
            Err("Starts at is required".to_string())
        );
        assert!(parse_moment("20/10/2026", "Starts at").is_err());
        assert!(parse_moment("not-a-date", "Expires at").is_err());
        assert_eq!(parse_optional_moment("  ", "Expires at"), Ok(None));
        assert_eq!(
            parse_optional_moment("2026-10-20", "Expires at"),
            Ok(Some(moment(2026, 10, 20, 0, 0)))
        );
    }

    #[test]
    fn staff_side_roles_see_everything() {
        for role in ["admin", "teacher", "staff"] {
            assert!(is_staff_side_role(role), "{role} should be staff-side");
        }
        for role in ["parent", "student", "", "superadmin"] {
            assert!(!is_staff_side_role(role), "{role} should be family-side");
        }
    }

    #[test]
    fn expiry_note_marks_expired_vs_upcoming() {
        let now = moment(2026, 10, 16, 12, 0);
        assert_eq!(expiry_note(None, now), None);
        assert_eq!(
            expiry_note(Some(moment(2026, 10, 20, 0, 0)), now),
            Some("Expires 20 Oct 2026".to_string())
        );
        assert_eq!(
            expiry_note(Some(moment(2026, 10, 10, 0, 0)), now),
            Some("Expired 10 Oct 2026".to_string())
        );
    }

    #[test]
    fn audience_pills_cover_every_audience() {
        assert_eq!(audience_class("all"), "pill-info");
        assert_eq!(audience_class("parents"), "pill-ok");
        assert_eq!(audience_class("staff"), "pill-warn");
        assert_eq!(audience_class("anything-else"), "pill-info");
    }
}
