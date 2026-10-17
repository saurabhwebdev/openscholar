//! Settings, grading scales, and semesters.
//!
//! Runtime SQL only. Money in BIGINT cents where applicable.

use chrono::{NaiveDate, Utc};
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GradingScaleRow {
    pub id: i64,
    pub name: String,
    pub min_score: f64,
    pub max_score: f64,
    pub grade: String,
    pub points: Option<f64>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SemesterRow {
    pub id: i64,
    pub school_year_id: i64,
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub is_current: bool,
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// `key` must be non-empty and at most 64 characters.
pub fn validate_setting(key: &str) -> Result<(), String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("Setting key is required".to_string());
    }
    if k.len() > 64 {
        return Err("Setting key must be at most 64 characters".to_string());
    }
    Ok(())
}

/// `name` non-empty; `min_score >= 0`; `max_score >= min_score`; `grade` non-empty.
pub fn validate_scale(name: &str, min_score: f64, max_score: f64, grade: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Scale name is required".to_string());
    }
    if min_score < 0.0 {
        return Err("Minimum score must be at least 0".to_string());
    }
    if max_score < min_score {
        return Err("Maximum score must be at least the minimum".to_string());
    }
    if grade.trim().is_empty() {
        return Err("Grade is required".to_string());
    }
    Ok(())
}

/// `name` non-empty; `end_date >= start_date`.
pub fn validate_semester(name: &str, start_date: NaiveDate, end_date: NaiveDate) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Semester name is required".to_string());
    }
    if end_date < start_date {
        return Err("End date must be on or after the start date".to_string());
    }
    Ok(())
}

// string-friendly helpers
pub fn validate_scale_str(name: &str, min_score: &str, max_score: &str, grade: &str) -> Result<(), String> {
    let min: f64 = min_score.trim().parse().map_err(|_| "Minimum score must be a number".to_string())?;
    let max: f64 = max_score.trim().parse().map_err(|_| "Maximum score must be a number".to_string())?;
    validate_scale(name, min, max, grade)
}

pub fn validate_semester_str(name: &str, start_date: &str, end_date: &str) -> Result<(), String> {
    let start = NaiveDate::parse_from_str(start_date.trim(), "%Y-%m-%d")
        .map_err(|_| "Start date must be YYYY-MM-DD".to_string())?;
    let end = NaiveDate::parse_from_str(end_date.trim(), "%Y-%m-%d")
        .map_err(|_| "End date must be YYYY-MM-DD".to_string())?;
    validate_semester(name, start, end)
}

// ---------------------------------------------------------------------------
// Pure grading helpers
// ---------------------------------------------------------------------------

/// Returns the grade label for `score` using the scale with the highest
/// `min_score` that is `<= score`.
pub fn grade_for_score(scales: &[GradingScaleRow], score: f64) -> Option<String> {
    scales
        .iter()
        .filter(|s| s.min_score <= score)
        .max_by(|a, b| a.min_score.partial_cmp(&b.min_score).unwrap_or(std::cmp::Ordering::Equal))
        .map(|s| s.grade.clone())
}

/// Points associated with the grade matched for `score`.
pub fn grade_points(scales: &[GradingScaleRow], score: f64) -> Option<f64> {
    let grade = grade_for_score(scales, score)?;
    scales
        .iter()
        .filter(|s| s.grade == grade && s.min_score <= score)
        .max_by(|a, b| a.min_score.partial_cmp(&b.min_score).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|s| s.points)
}

/// Points for a given grade label.
pub fn points_for_grade(scales: &[GradingScaleRow], grade: &str) -> Option<f64> {
    scales
        .iter()
        .find(|s| s.grade == grade)
        .and_then(|s| s.points)
}

// aliases
pub fn grade_points_for_score(scales: &[GradingScaleRow], score: f64) -> Option<f64> {
    grade_points(scales, score)
}

// ---------------------------------------------------------------------------
// Settings queries
// ---------------------------------------------------------------------------

pub async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<SettingRow>, AppError> {
    let row = sqlx::query_as::<_, SettingRow>(
        "SELECT key, value, updated_at
         FROM settings WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_setting_value(pool: &PgPool, key: &str) -> Result<Option<String>, AppError> {
    let v: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(v)
}

pub async fn set_setting(pool: &PgPool, key: &str, value: &str) -> Result<SettingRow, AppError> {
    let row = sqlx::query_as::<_, SettingRow>(
        "INSERT INTO settings (key, value) VALUES ($1, $2)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()
         RETURNING key, value, updated_at",
    )
    .bind(key)
    .bind(value)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list_settings(pool: &PgPool) -> Result<Vec<SettingRow>, AppError> {
    let rows = sqlx::query_as::<_, SettingRow>(
        "SELECT key, value, updated_at FROM settings ORDER BY key ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_setting(pool: &PgPool, key: &str) -> Result<(), AppError> {
    let done = sqlx::query("DELETE FROM settings WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Grading scales CRUD
// ---------------------------------------------------------------------------

pub async fn list_scales(pool: &PgPool) -> Result<Vec<GradingScaleRow>, AppError> {
    let rows = sqlx::query_as::<_, GradingScaleRow>(
        "SELECT id, name, min_score::FLOAT8 AS min_score, max_score::FLOAT8 AS max_score,
                grade, points::FLOAT8 AS points, created_at
         FROM grading_scales ORDER BY min_score DESC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_scales(pool: &PgPool) -> Result<Vec<GradingScaleRow>, AppError> {
    list_scales(pool).await
}

pub async fn find_scale(pool: &PgPool, id: i64) -> Result<Option<GradingScaleRow>, AppError> {
    let row = sqlx::query_as::<_, GradingScaleRow>(
        "SELECT id, name, min_score::FLOAT8 AS min_score, max_score::FLOAT8 AS max_score,
                grade, points::FLOAT8 AS points, created_at
         FROM grading_scales WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_scale(pool: &PgPool, id: i64) -> Result<Option<GradingScaleRow>, AppError> {
    find_scale(pool, id).await
}

pub async fn scale_row(pool: &PgPool, id: i64) -> Result<GradingScaleRow, AppError> {
    find_scale(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn create_scale(
    pool: &PgPool,
    name: &str,
    min_score: f64,
    max_score: f64,
    grade: &str,
    points: Option<f64>,
) -> Result<GradingScaleRow, AppError> {
    let row = sqlx::query_as::<_, GradingScaleRow>(
        "INSERT INTO grading_scales (name, min_score, max_score, grade, points)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, min_score::FLOAT8 AS min_score, max_score::FLOAT8 AS max_score,
                   grade, points::FLOAT8 AS points, created_at",
    )
    .bind(name.trim())
    .bind(min_score)
    .bind(max_score)
    .bind(grade.trim())
    .bind(points)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_scale(
    pool: &PgPool,
    id: i64,
    name: &str,
    min_score: f64,
    max_score: f64,
    grade: &str,
    points: Option<f64>,
) -> Result<GradingScaleRow, AppError> {
    let row = sqlx::query_as::<_, GradingScaleRow>(
        "UPDATE grading_scales
         SET name = $2, min_score = $3, max_score = $4, grade = $5, points = $6
         WHERE id = $1
         RETURNING id, name, min_score::FLOAT8 AS min_score, max_score::FLOAT8 AS max_score,
                   grade, points::FLOAT8 AS points, created_at",
    )
    .bind(id)
    .bind(name.trim())
    .bind(min_score)
    .bind(max_score)
    .bind(grade.trim())
    .bind(points)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn delete_scale(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let done = sqlx::query("DELETE FROM grading_scales WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_scale(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_scale(pool, id).await
}

// ---------------------------------------------------------------------------
// Semesters CRUD
// ---------------------------------------------------------------------------

pub async fn list_semesters(pool: &PgPool, school_year_id: i64) -> Result<Vec<SemesterRow>, AppError> {
    let rows = sqlx::query_as::<_, SemesterRow>(
        "SELECT id, school_year_id, name, start_date, end_date, is_current
         FROM semesters WHERE school_year_id = $1 ORDER BY start_date, name",
    )
    .bind(school_year_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_semesters_by_year(pool: &PgPool, school_year_id: i64) -> Result<Vec<SemesterRow>, AppError> {
    list_semesters(pool, school_year_id).await
}

pub async fn semesters_for_year(pool: &PgPool, school_year_id: i64) -> Result<Vec<SemesterRow>, AppError> {
    list_semesters(pool, school_year_id).await
}

pub async fn list_all_semesters(pool: &PgPool) -> Result<Vec<SemesterRow>, AppError> {
    let rows = sqlx::query_as::<_, SemesterRow>(
        "SELECT id, school_year_id, name, start_date, end_date, is_current
         FROM semesters ORDER BY school_year_id, start_date, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_semesters(pool: &PgPool) -> Result<Vec<SemesterRow>, AppError> {
    list_all_semesters(pool).await
}

pub async fn list_semesters_all(pool: &PgPool) -> Result<Vec<SemesterRow>, AppError> {
    list_all_semesters(pool).await
}

pub async fn find_semester(pool: &PgPool, id: i64) -> Result<Option<SemesterRow>, AppError> {
    let row = sqlx::query_as::<_, SemesterRow>(
        "SELECT id, school_year_id, name, start_date, end_date, is_current
         FROM semesters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_semester(pool: &PgPool, id: i64) -> Result<Option<SemesterRow>, AppError> {
    find_semester(pool, id).await
}

pub async fn semester_row(pool: &PgPool, id: i64) -> Result<SemesterRow, AppError> {
    find_semester(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn create_semester(
    pool: &PgPool,
    school_year_id: i64,
    name: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<SemesterRow, AppError> {
    let row = sqlx::query_as::<_, SemesterRow>(
        "INSERT INTO semesters (school_year_id, name, start_date, end_date)
         VALUES ($1, $2, $3, $4)
         RETURNING id, school_year_id, name, start_date, end_date, is_current",
    )
    .bind(school_year_id)
    .bind(name.trim())
    .bind(start_date)
    .bind(end_date)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_semester(
    pool: &PgPool,
    id: i64,
    school_year_id: i64,
    name: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<SemesterRow, AppError> {
    let row = sqlx::query_as::<_, SemesterRow>(
        "UPDATE semesters
         SET school_year_id = $2, name = $3, start_date = $4, end_date = $5
         WHERE id = $1
         RETURNING id, school_year_id, name, start_date, end_date, is_current",
    )
    .bind(id)
    .bind(school_year_id)
    .bind(name.trim())
    .bind(start_date)
    .bind(end_date)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

/// Clear is_current for all semesters in the same year, then set chosen.
pub async fn set_current_semester(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let sem = semester_row(pool, id).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE semesters SET is_current = FALSE WHERE school_year_id = $1")
        .bind(sem.school_year_id)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("UPDATE semesters SET is_current = TRUE WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn set_current(pool: &PgPool, id: i64) -> Result<(), AppError> {
    set_current_semester(pool, id).await
}

pub async fn delete_semester(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let done = sqlx::query("DELETE FROM semesters WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_semester(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_semester(pool, id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_setting_key() {
        assert!(validate_setting("").is_err());
        assert!(validate_setting("   ").is_err());
        assert!(validate_setting(&"a".repeat(65)).is_err());
        assert!(validate_setting("school_name").is_ok());
        assert!(validate_setting(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn validate_scale_rules() {
        assert!(validate_scale("", 0.0, 100.0, "A").is_err());
        assert!(validate_scale("S", -1.0, 100.0, "A").is_err());
        assert!(validate_scale("S", 50.0, 40.0, "A").is_err());
        assert!(validate_scale("S", 0.0, 100.0, "").is_err());
        assert!(validate_scale("S", 0.0, 100.0, "A").is_ok());
    }

    #[test]
    fn validate_semester_rules() {
        use chrono::NaiveDate;
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        assert!(validate_semester("", start, start).is_err());
        assert!(validate_semester("Fall", start, end).is_err());
        assert!(validate_semester("Fall", end, start).is_ok());
    }

    #[test]
    fn grade_for_score_picks_highest_min() {
        let scales = vec![
            GradingScaleRow { id: 1, name: "S".into(), min_score: 0.0, max_score: 59.99, grade: "F".into(), points: Some(0.0), created_at: chrono::DateTime::<Utc>::from_timestamp(0,0).unwrap() },
            GradingScaleRow { id: 2, name: "S".into(), min_score: 60.0, max_score: 74.99, grade: "C".into(), points: Some(2.0), created_at: chrono::DateTime::<Utc>::from_timestamp(0,0).unwrap() },
            GradingScaleRow { id: 3, name: "S".into(), min_score: 75.0, max_score: 100.0, grade: "A".into(), points: Some(4.0), created_at: chrono::DateTime::<Utc>::from_timestamp(0,0).unwrap() },
        ];
        assert_eq!(grade_for_score(&scales, 72.0), Some("C".to_string()));
        assert_eq!(grade_for_score(&scales, 90.0), Some("A".to_string()));
        assert_eq!(grade_for_score(&scales, -1.0), None);
        assert_eq!(grade_points(&scales, 90.0), Some(4.0));
        assert_eq!(grade_points(&scales, 10.0), Some(0.0));
    }
}
