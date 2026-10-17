use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

/// All valid attendance statuses (mirrors the DB CHECK constraint).
pub const STATUSES: &[&str] = &["present", "absent", "late", "excused"];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AttendanceRow {
    pub id: i64,
    pub student_id: i64,
    pub class_id: i64,
    pub date: NaiveDate,
    pub status: String,
    pub note: Option<String>,
    pub recorded_by: Option<i64>,
}

/// One roster line for the mark-attendance page: the enrolled student plus
/// any attendance already recorded for the selected date.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RosterEntry {
    pub student_id: i64,
    pub admission_no: String,
    pub full_name: String,
    pub status: Option<String>,
    pub note: Option<String>,
}

/// Class `<select>` option with its school-year context.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassPickerOption {
    pub id: i64,
    pub name: String,
    pub year_name: String,
    pub is_current: bool,
}

/// Per-day totals for the history strip.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DaySummary {
    pub date: NaiveDate,
    pub total: i64,
    pub present: i64,
    pub absent: i64,
}

/// One parsed per-student submission: status plus optional note.
#[derive(Debug, Clone, PartialEq)]
pub struct AttendanceEntry {
    pub student_id: i64,
    pub status: String,
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Accept only the four statuses allowed by the DB CHECK constraint.
pub fn validate_status(status: &str) -> Result<(), String> {
    let s = status.trim();
    if STATUSES.contains(&s) {
        Ok(())
    } else {
        Err("Status must be present, absent, late, or excused".to_string())
    }
}

/// Parse a `YYYY-MM-DD` form/query date.
pub fn validate_date(date: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map_err(|_| "Date must be YYYY-MM-DD".to_string())
}

/// Validate a clear-day request: `class_id` must be positive and `date`
/// must parse as `YYYY-MM-DD`. Returns the parsed date for the `DELETE`.
pub fn validate_clear_day(class_id: i64, date_str: &str) -> Result<NaiveDate, String> {
    if class_id <= 0 {
        return Err("Choose a class first".to_string());
    }
    validate_date(date_str)
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only)
// ---------------------------------------------------------------------------

/// All classes, current-year classes first, each labelled with its year name.
pub async fn classes_for_picker(pool: &PgPool) -> Result<Vec<ClassPickerOption>, AppError> {
    let rows = sqlx::query_as::<_, ClassPickerOption>(
        "SELECT c.id, c.name, y.name AS year_name, y.is_current
         FROM classes c
         JOIN school_years y ON y.id = c.school_year_id
         ORDER BY y.is_current DESC, y.start_date DESC, c.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Enrolled active students with any attendance recorded for `date`.
pub async fn roster_for(
    pool: &PgPool,
    class_id: i64,
    date: NaiveDate,
) -> Result<Vec<RosterEntry>, AppError> {
    let rows = sqlx::query_as::<_, RosterEntry>(
        "SELECT s.id AS student_id, s.admission_no,
                (s.first_name || ' ' || s.last_name) AS full_name,
                a.status, a.note
         FROM enrollments e
         JOIN students s ON s.id = e.student_id
         LEFT JOIN attendance a
           ON a.student_id = s.id AND a.class_id = $1 AND a.date = $2
         WHERE e.class_id = $1 AND s.status = 'active'
         ORDER BY s.last_name, s.first_name",
    )
    .bind(class_id)
    .bind(date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Upsert one row per student; rows with a blank/zero student id are skipped
/// defensively so malformed `status_` keys can never insert garbage.
pub async fn save_attendance(
    pool: &PgPool,
    class_id: i64,
    date: NaiveDate,
    entries: &[AttendanceEntry],
    recorded_by: i64,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    for entry in entries {
        if entry.student_id <= 0 {
            continue;
        }
        validate_status(&entry.status).map_err(AppError::BadRequest)?;
        let note = entry
            .note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        sqlx::query(
            "INSERT INTO attendance (student_id, class_id, date, status, note, recorded_by)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (student_id, class_id, date)
             DO UPDATE SET status = EXCLUDED.status, note = EXCLUDED.note,
                           recorded_by = EXCLUDED.recorded_by",
        )
        .bind(entry.student_id)
        .bind(class_id)
        .bind(date)
        .bind(entry.status.trim())
        .bind(note)
        .bind(recorded_by)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Delete every attendance row for one class on one date.
/// Returns the number of rows removed.
pub async fn clear_day(
    pool: &PgPool,
    class_id: i64,
    date: NaiveDate,
) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM attendance WHERE class_id = $1 AND date = $2")
        .bind(class_id)
        .bind(date)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Most recent recorded days for a class with present/absent counts.
pub async fn recent_days(
    pool: &PgPool,
    class_id: i64,
    limit: i64,
) -> Result<Vec<DaySummary>, AppError> {
    let rows = sqlx::query_as::<_, DaySummary>(
        "SELECT date,
                COUNT(*)::BIGINT AS total,
                COUNT(*) FILTER (WHERE status = 'present')::BIGINT AS present,
                COUNT(*) FILTER (WHERE status = 'absent')::BIGINT AS absent
         FROM attendance
         WHERE class_id = $1
         GROUP BY date
         ORDER BY date DESC
         LIMIT $2",
    )
    .bind(class_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_all_valid_statuses() {
        for s in ["present", "absent", "late", "excused"] {
            assert!(validate_status(s).is_ok(), "{s} should be valid");
        }
    }

    #[test]
    fn trims_status_before_checking() {
        assert!(validate_status("  present  ").is_ok());
    }

    #[test]
    fn rejects_unknown_status() {
        assert_eq!(
            validate_status("sick"),
            Err("Status must be present, absent, late, or excused".to_string())
        );
    }

    #[test]
    fn rejects_blank_status() {
        assert!(validate_status("").is_err());
        assert!(validate_status("   ").is_err());
    }

    #[test]
    fn rejects_wrong_case_status() {
        assert!(validate_status("Present").is_err());
        assert!(validate_status("ABSENT").is_err());
    }

    #[test]
    fn accepts_valid_date() {
        let d = validate_date("2026-10-16").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 10, 16).unwrap());
    }

    #[test]
    fn trims_date_before_parsing() {
        assert!(validate_date("  2026-01-05  ").is_ok());
    }

    #[test]
    fn rejects_bad_date_format() {
        assert_eq!(
            validate_date("16/10/2026"),
            Err("Date must be YYYY-MM-DD".to_string())
        );
    }

    #[test]
    fn rejects_impossible_date() {
        assert_eq!(
            validate_date("2026-02-30"),
            Err("Date must be YYYY-MM-DD".to_string())
        );
    }

    #[test]
    fn rejects_blank_date() {
        assert!(validate_date("").is_err());
        assert!(validate_date("   ").is_err());
    }

    #[test]
    fn clear_day_accepts_valid_class_and_date() {
        let d = validate_clear_day(3, "2026-10-16").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 10, 16).unwrap());
    }

    #[test]
    fn clear_day_rejects_missing_class() {
        assert_eq!(
            validate_clear_day(0, "2026-10-16"),
            Err("Choose a class first".to_string())
        );
        assert_eq!(
            validate_clear_day(-2, "2026-10-16"),
            Err("Choose a class first".to_string())
        );
    }

    #[test]
    fn clear_day_rejects_bad_date() {
        assert_eq!(
            validate_clear_day(3, "16/10/2026"),
            Err("Date must be YYYY-MM-DD".to_string())
        );
        assert!(validate_clear_day(3, "").is_err());
    }

    #[test]
    fn clear_day_trims_date_before_parsing() {
        assert!(validate_clear_day(3, "  2026-01-05  ").is_ok());
    }
}
