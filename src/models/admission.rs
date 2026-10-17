//! Admissions module: applicant row types, validation, and runtime-SQL queries.
//!
//! All SQL here is runtime-checked (`sqlx::query` / `query_as`); no
//! compile-time macros beyond `#[derive(sqlx::FromRow)]`.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

/// Full `applicants` row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApplicantRow {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: Option<NaiveDate>,
    pub gender: Option<String>,
    pub guardian_name: Option<String>,
    pub guardian_phone: Option<String>,
    pub guardian_email: Option<String>,
    pub address: Option<String>,
    pub applying_for: String,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Template-friendly applicant with dates preformatted as `YYYY-MM-DD`
/// strings (empty string when unknown) so Askama templates do no date logic.
#[derive(Debug, Clone)]
pub struct ApplicantView {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub full_name: String,
    pub date_of_birth: String,
    pub gender: String,
    pub guardian_name: String,
    pub guardian_phone: String,
    pub guardian_email: String,
    pub address: String,
    pub applying_for: String,
    pub status: String,
    pub notes: String,
}

impl From<ApplicantRow> for ApplicantView {
    fn from(row: ApplicantRow) -> Self {
        let full_name = format!("{} {}", row.first_name, row.last_name)
            .trim()
            .to_string();
        Self {
            id: row.id,
            first_name: row.first_name,
            last_name: row.last_name,
            full_name,
            date_of_birth: row.date_of_birth.map(|d| d.to_string()).unwrap_or_default(),
            gender: row.gender.unwrap_or_default(),
            guardian_name: row.guardian_name.unwrap_or_default(),
            guardian_phone: row.guardian_phone.unwrap_or_default(),
            guardian_email: row.guardian_email.unwrap_or_default(),
            address: row.address.unwrap_or_default(),
            applying_for: row.applying_for,
            status: row.status,
            notes: row.notes.unwrap_or_default(),
        }
    }
}

/// Validated, insert-ready applicant payload. Empty form strings must already
/// be normalised (`None`); `status` is always `new` on create.
#[derive(Debug, Clone)]
pub struct ApplicantInput {
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: Option<NaiveDate>,
    pub gender: Option<String>,
    pub guardian_name: Option<String>,
    pub guardian_phone: Option<String>,
    pub guardian_email: Option<String>,
    pub address: Option<String>,
    pub applying_for: String,
    pub notes: Option<String>,
}

pub const ALLOWED_GENDERS: &[&str] = &["male", "female", "other"];
pub const ALLOWED_STATUSES: &[&str] = &["new", "reviewing", "accepted", "rejected", "enrolled"];

/// Pure validation for the new-applicant form.
///
/// Rules: `first_name` / `last_name` must be non-empty (after trimming);
/// `gender` must be empty or one of male/female/other.
pub fn validate_applicant(
    first_name: &str,
    last_name: &str,
    gender: Option<&str>,
) -> Result<(), String> {
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
    Ok(())
}

/// Is a status transition allowed?
///
/// - `new` -> `reviewing` / `rejected`
/// - `reviewing` -> `accepted` / `rejected`
/// - `accepted` -> `enrolled`
/// - `enrolled` (and `rejected`) are terminal: no outgoing transitions.
pub fn can_transition(from: &str, to: &str) -> bool {
    matches!(
        (from.trim(), to.trim()),
        ("new", "reviewing")
            | ("new", "rejected")
            | ("reviewing", "accepted")
            | ("reviewing", "rejected")
            | ("accepted", "enrolled")
    )
}

/// Next statuses offered as buttons on the detail page.
pub fn next_statuses(status: &str) -> Vec<&'static str> {
    match status.trim() {
        "new" => vec!["reviewing", "rejected"],
        "reviewing" => vec!["accepted", "rejected"],
        "accepted" => vec!["enrolled"],
        _ => vec![],
    }
}

pub async fn find_applicant(pool: &PgPool, id: i64) -> Result<Option<ApplicantRow>, AppError> {
    let row = sqlx::query_as::<_, ApplicantRow>(concat!(
        "SELECT ",
        "id, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, status, notes, created_at",
        " FROM applicants WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_applicants(pool: &PgPool) -> Result<Vec<ApplicantRow>, AppError> {
    let rows = sqlx::query_as::<_, ApplicantRow>(concat!(
        "SELECT ",
        "id, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, status, notes, created_at",
        " FROM applicants ORDER BY created_at DESC LIMIT 500"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_by_status(pool: &PgPool, status: &str) -> Result<Vec<ApplicantRow>, AppError> {
    let rows = sqlx::query_as::<_, ApplicantRow>(concat!(
        "SELECT ",
        "id, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, status, notes, created_at",
        " FROM applicants WHERE status = $1 ORDER BY created_at DESC LIMIT 500"
    ))
    .bind(status.trim())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Per-status applicant counts for the pipeline header pills.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

pub async fn applicant_counts(pool: &PgPool) -> Result<Vec<StatusCount>, AppError> {
    let rows = sqlx::query_as::<_, StatusCount>(
        "SELECT status, COUNT(*) AS count FROM applicants GROUP BY status",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Count for one status (convenience over `applicant_counts`).
pub fn count_for(counts: &[StatusCount], status: &str) -> i64 {
    counts
        .iter()
        .find(|c| c.status == status)
        .map(|c| c.count)
        .unwrap_or(0)
}

pub async fn create_applicant(
    pool: &PgPool,
    input: &ApplicantInput,
) -> Result<ApplicantRow, AppError> {
    let row = sqlx::query_as::<_, ApplicantRow>(
        "INSERT INTO applicants (first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, notes)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, status, notes, created_at",
    )
    .bind(input.first_name.trim())
    .bind(input.last_name.trim())
    .bind(input.date_of_birth)
    .bind(input.gender.clone())
    .bind(input.guardian_name.clone())
    .bind(input.guardian_phone.clone())
    .bind(input.guardian_email.clone())
    .bind(input.address.clone())
    .bind(input.applying_for.trim())
    .bind(input.notes.clone())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Atomically turn an accepted applicant into a student: locks the applicant
/// row, verifies it is still `accepted` (so two concurrent enrols — or a retry
/// after a half-failure — can't create a second student), inserts the student,
/// and flips the applicant to `enrolled`, all in one transaction.
pub async fn enroll_applicant(
    pool: &PgPool,
    applicant_id: i64,
    input: &crate::models::student::StudentInput,
) -> Result<i64, AppError> {
    let mut tx = pool.begin().await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM applicants WHERE id = $1 FOR UPDATE")
            .bind(applicant_id)
            .fetch_optional(&mut *tx)
            .await?;
    match status {
        None => return Err(AppError::NotFound),
        Some(s) if s != "accepted" => {
            return Err(AppError::BadRequest(
                "Only accepted applicants can be enrolled".to_string(),
            ));
        }
        Some(_) => {}
    }
    // Mirrors student::create_student's INSERT (kept here so it shares the tx).
    let student_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO students (admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING id",
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
    .fetch_one(&mut *tx)
    .await;
    let student_id = match student_id {
        Ok(id) => id,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::BadRequest(
                "That admission number is already taken".to_string(),
            ));
        }
        Err(e) => return Err(AppError::Db(e)),
    };
    sqlx::query("UPDATE applicants SET status = 'enrolled' WHERE id = $1")
        .bind(applicant_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(student_id)
}

pub async fn update_applicant_status(
    pool: &PgPool,
    id: i64,
    status: &str,
) -> Result<ApplicantRow, AppError> {
    let row = sqlx::query_as::<_, ApplicantRow>(
        "UPDATE applicants SET status = $2 WHERE id = $1
         RETURNING id, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, applying_for, status, notes, created_at",
    )
    .bind(id)
    .bind(status.trim())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_applicant() {
        assert!(validate_applicant("Ada", "Lovelace", Some("female")).is_ok());
    }

    #[test]
    fn accepts_minimal_applicant_with_no_gender() {
        assert!(validate_applicant("Alan", "Turing", None).is_ok());
        assert!(validate_applicant("Grace", "Hopper", Some("")).is_ok());
    }

    #[test]
    fn rejects_empty_first_name() {
        assert!(validate_applicant("", "Lovelace", None).is_err());
        assert!(validate_applicant("   ", "Lovelace", None).is_err());
    }

    #[test]
    fn rejects_empty_last_name() {
        assert!(validate_applicant("Ada", "", None).is_err());
        assert!(validate_applicant("Ada", "   ", None).is_err());
    }

    #[test]
    fn rejects_unknown_gender() {
        assert!(validate_applicant("Ada", "Lovelace", Some("unknown")).is_err());
    }

    #[test]
    fn allows_new_to_reviewing_or_rejected() {
        assert!(can_transition("new", "reviewing"));
        assert!(can_transition("new", "rejected"));
        assert!(!can_transition("new", "accepted"));
        assert!(!can_transition("new", "enrolled"));
        assert!(!can_transition("new", "new"));
    }

    #[test]
    fn allows_reviewing_to_accepted_or_rejected() {
        assert!(can_transition("reviewing", "accepted"));
        assert!(can_transition("reviewing", "rejected"));
        assert!(!can_transition("reviewing", "enrolled"));
        assert!(!can_transition("reviewing", "new"));
    }

    #[test]
    fn allows_accepted_to_enrolled_only() {
        assert!(can_transition("accepted", "enrolled"));
        assert!(!can_transition("accepted", "rejected"));
        assert!(!can_transition("accepted", "reviewing"));
        assert!(!can_transition("accepted", "new"));
    }

    #[test]
    fn enrolled_is_terminal() {
        for to in ["new", "reviewing", "accepted", "rejected", "enrolled"] {
            assert!(!can_transition("enrolled", to), "enrolled -> {to}");
        }
    }

    #[test]
    fn rejected_is_terminal() {
        for to in ["new", "reviewing", "accepted", "rejected", "enrolled"] {
            assert!(!can_transition("rejected", to), "rejected -> {to}");
        }
    }

    #[test]
    fn rejects_unknown_statuses() {
        assert!(!can_transition("new", "graduated"));
        assert!(!can_transition("archived", "new"));
        assert!(!can_transition("", "new"));
    }

    #[test]
    fn view_preformats_dates_and_full_name() {
        let row = ApplicantRow {
            id: 3,
            first_name: "Ada".to_string(),
            last_name: "Lovelace".to_string(),
            date_of_birth: NaiveDate::from_ymd_opt(2015, 12, 10),
            gender: Some("female".to_string()),
            guardian_name: None,
            guardian_phone: None,
            guardian_email: None,
            address: None,
            applying_for: "Grade 6".to_string(),
            status: "new".to_string(),
            notes: None,
            created_at: chrono::DateTime::from_naive_utc_and_offset(
                chrono::NaiveDateTime::MIN,
                chrono::Utc,
            ),
        };
        let view = ApplicantView::from(row);
        assert_eq!(view.full_name, "Ada Lovelace");
        assert_eq!(view.date_of_birth, "2015-12-10");
        assert_eq!(view.guardian_name, "");
        assert_eq!(view.status, "new");
    }

    #[test]
    fn count_for_returns_zero_for_missing_status() {
        let counts = vec![StatusCount {
            status: "new".to_string(),
            count: 2,
        }];
        assert_eq!(count_for(&counts, "new"), 2);
        assert_eq!(count_for(&counts, "reviewing"), 0);
    }

    #[test]
    fn next_statuses_match_transitions() {
        assert_eq!(next_statuses("new"), vec!["reviewing", "rejected"]);
        assert_eq!(next_statuses("reviewing"), vec!["accepted", "rejected"]);
        assert_eq!(next_statuses("accepted"), vec!["enrolled"]);
        assert!(next_statuses("enrolled").is_empty());
        assert!(next_statuses("rejected").is_empty());
    }
}
