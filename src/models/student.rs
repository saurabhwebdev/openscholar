//! Students module: row types, validation, and runtime-SQL queries.
//!
//! All SQL here is runtime-checked (`sqlx::query` / `query_as`); no
//! compile-time macros beyond `#[derive(sqlx::FromRow)]`.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

/// Full `students` row. Nullable dates use `Option`; `enrolled_on` has a DB
/// default so it is always present.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StudentRow {
    pub id: i64,
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: Option<NaiveDate>,
    pub gender: Option<String>,
    pub guardian_name: Option<String>,
    pub guardian_phone: Option<String>,
    pub guardian_email: Option<String>,
    pub address: Option<String>,
    pub status: String,
    pub enrolled_on: NaiveDate,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Template-friendly student with dates preformatted as `YYYY-MM-DD` strings
/// (empty string when unknown) so Askama templates do no date logic.
#[derive(Debug, Clone)]
pub struct StudentView {
    pub id: i64,
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
    pub full_name: String,
    pub date_of_birth: String,
    pub gender: String,
    pub guardian_name: String,
    pub guardian_phone: String,
    pub guardian_email: String,
    pub address: String,
    pub status: String,
    pub enrolled_on: String,
}

impl From<StudentRow> for StudentView {
    fn from(row: StudentRow) -> Self {
        let full_name = format!("{} {}", row.first_name, row.last_name)
            .trim()
            .to_string();
        Self {
            id: row.id,
            admission_no: row.admission_no,
            first_name: row.first_name,
            last_name: row.last_name,
            full_name,
            date_of_birth: row.date_of_birth.map(|d| d.to_string()).unwrap_or_default(),
            gender: row.gender.unwrap_or_default(),
            guardian_name: row.guardian_name.unwrap_or_default(),
            guardian_phone: row.guardian_phone.unwrap_or_default(),
            guardian_email: row.guardian_email.unwrap_or_default(),
            address: row.address.unwrap_or_default(),
            status: row.status,
            enrolled_on: row.enrolled_on.to_string(),
        }
    }
}

/// Validated, insert-ready student payload. Empty form strings must already be
/// normalised (`None`) except `status`, which must be a valid status value.
#[derive(Debug, Clone)]
pub struct StudentInput {
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: Option<NaiveDate>,
    pub gender: Option<String>,
    pub guardian_name: Option<String>,
    pub guardian_phone: Option<String>,
    pub guardian_email: Option<String>,
    pub address: Option<String>,
    pub status: String,
    pub enrolled_on: NaiveDate,
}

pub const ALLOWED_GENDERS: &[&str] = &["male", "female", "other"];
pub const ALLOWED_STATUSES: &[&str] = &["active", "inactive", "graduated"];

/// Pure validation shared by create and update.
///
/// Rules: `admission_no` / `first_name` / `last_name` must be non-empty
/// (after trimming); `gender` must be empty or one of male/female/other;
/// `status` must be one of active/inactive/graduated.
pub fn validate_student(
    admission_no: &str,
    first_name: &str,
    last_name: &str,
    gender: Option<&str>,
    status: &str,
) -> Result<(), String> {
    if admission_no.trim().is_empty() {
        return Err("Admission number is required".to_string());
    }
    if first_name.trim().is_empty() {
        return Err("First name is required".to_string());
    }
    if last_name.trim().is_empty() {
        return Err("Last name is required".to_string());
    }
    match gender.map(str::trim) {
        None | Some("") => {}
        Some(g) if ALLOWED_GENDERS.contains(&g) => {}
        _ => return Err("Gender must be male, female, or other".to_string()),
    }
    if !ALLOWED_STATUSES.contains(&status.trim()) {
        return Err("Status must be active, inactive, or graduated".to_string());
    }
    Ok(())
}

pub async fn find_student(pool: &PgPool, id: i64) -> Result<Option<StudentRow>, AppError> {
    let row = sqlx::query_as::<_, StudentRow>(concat!(
        "SELECT ",
        "id, admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on, created_at",
        " FROM students WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn count_students(pool: &PgPool) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM students")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn list_students(pool: &PgPool) -> Result<Vec<StudentRow>, AppError> {
    list_students_paged(pool, 200, 0).await
}

pub async fn list_students_paged(pool: &PgPool, limit: i64, offset: i64) -> Result<Vec<StudentRow>, AppError> {
    let rows = sqlx::query_as::<_, StudentRow>(concat!(
        "SELECT ",
        "id, admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on, created_at",
        " FROM students ORDER BY last_name, first_name LIMIT $1 OFFSET $2"
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn count_search_students(pool: &PgPool, q: &str) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM students WHERE lower(first_name || ' ' || last_name || ' ' || admission_no) LIKE '%' || lower($1) || '%'"
    )
    .bind(q.trim())
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn search_students(pool: &PgPool, q: &str) -> Result<Vec<StudentRow>, AppError> {
    search_students_paged(pool, q, 200, 0).await
}

pub async fn search_students_paged(pool: &PgPool, q: &str, limit: i64, offset: i64) -> Result<Vec<StudentRow>, AppError> {
    let rows = sqlx::query_as::<_, StudentRow>(concat!(
        "SELECT ",
        "id, admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on, created_at",
        " FROM students",
        " WHERE lower(first_name || ' ' || last_name || ' ' || admission_no) LIKE '%' || lower($1) || '%'",
        " ORDER BY last_name, first_name LIMIT $2 OFFSET $3"
    ))
    .bind(q.trim())
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_student(pool: &PgPool, input: &StudentInput) -> Result<StudentRow, AppError> {
    let row = sqlx::query_as::<_, StudentRow>(
        "INSERT INTO students (admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING id, admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on, created_at",
    )
    .bind(input.admission_no.trim())
    .bind(input.first_name.trim())
    .bind(input.last_name.trim())
    .bind(input.date_of_birth)
    .bind(input.gender.clone())
    .bind(input.guardian_name.clone())
    .bind(input.guardian_phone.clone())
    .bind(input.guardian_email.clone())
    .bind(input.address.clone())
    .bind(input.status.trim())
    .bind(input.enrolled_on)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_student(
    pool: &PgPool,
    id: i64,
    input: &StudentInput,
) -> Result<StudentRow, AppError> {
    let row = sqlx::query_as::<_, StudentRow>(
        "UPDATE students SET admission_no = $2, first_name = $3, last_name = $4, date_of_birth = $5, gender = $6,
                guardian_name = $7, guardian_phone = $8, guardian_email = $9, address = $10, status = $11, enrolled_on = $12
         WHERE id = $1
         RETURNING id, admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on, created_at",
    )
    .bind(id)
    .bind(input.admission_no.trim())
    .bind(input.first_name.trim())
    .bind(input.last_name.trim())
    .bind(input.date_of_birth)
    .bind(input.gender.clone())
    .bind(input.guardian_name.clone())
    .bind(input.guardian_phone.clone())
    .bind(input.guardian_email.clone())
    .bind(input.address.clone())
    .bind(input.status.trim())
    .bind(input.enrolled_on)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// True when `err` is a Postgres unique-violation (e.g. duplicate
/// `admission_no`, code 23505) so routes can bounce back to the form.
pub fn is_duplicate_admission(err: &AppError) -> bool {
    match err {
        AppError::Db(sqlx::Error::Database(db)) => {
            db.code().as_deref() == Some("23505")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_fully_valid_student() {
        assert!(
            validate_student("ADM-001", "Ada", "Lovelace", Some("female"), "active").is_ok()
        );
    }

    #[test]
    fn accepts_minimal_student_with_no_gender() {
        assert!(validate_student("ADM-002", "Alan", "Turing", None, "inactive").is_ok());
        assert!(validate_student("ADM-003", "Grace", "Hopper", Some(""), "graduated").is_ok());
    }

    #[test]
    fn rejects_empty_first_name() {
        assert!(validate_student("ADM-001", "", "Lovelace", None, "active").is_err());
    }

    #[test]
    fn rejects_blank_first_name() {
        assert!(validate_student("ADM-001", "   ", "Lovelace", None, "active").is_err());
    }

    #[test]
    fn rejects_empty_last_name() {
        assert!(validate_student("ADM-001", "Ada", "", None, "active").is_err());
    }

    #[test]
    fn rejects_empty_admission_no() {
        assert!(validate_student("  ", "Ada", "Lovelace", None, "active").is_err());
    }

    #[test]
    fn rejects_unknown_gender() {
        assert!(validate_student("ADM-001", "Ada", "Lovelace", Some("unknown"), "active").is_err());
    }

    #[test]
    fn rejects_unknown_status() {
        assert!(validate_student("ADM-001", "Ada", "Lovelace", None, "suspended").is_err());
        assert!(validate_student("ADM-001", "Ada", "Lovelace", None, "").is_err());
    }

    #[test]
    fn view_preformats_dates_and_full_name() {
        let row = StudentRow {
            id: 1,
            admission_no: "ADM-001".to_string(),
            first_name: "Ada".to_string(),
            last_name: "Lovelace".to_string(),
            date_of_birth: NaiveDate::from_ymd_opt(2015, 12, 10),
            gender: Some("female".to_string()),
            guardian_name: None,
            guardian_phone: None,
            guardian_email: None,
            address: None,
            status: "active".to_string(),
            enrolled_on: NaiveDate::from_ymd_opt(2024, 9, 1).unwrap(),
            created_at: chrono::DateTime::from_naive_utc_and_offset(
                chrono::NaiveDateTime::MIN,
                chrono::Utc,
            ),
        };
        let view = StudentView::from(row);
        assert_eq!(view.full_name, "Ada Lovelace");
        assert_eq!(view.date_of_birth, "2015-12-10");
        assert_eq!(view.enrolled_on, "2024-09-01");
        assert_eq!(view.guardian_name, "");
    }
}
