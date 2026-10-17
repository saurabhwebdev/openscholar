//! Welfare module: behavior incidents + medical records, validation, runtime SQL.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentRow {
    pub id: i64,
    pub student_id: i64,
    pub incident_date: NaiveDate,
    pub kind: String,
    pub category: Option<String>,
    pub description: String,
    pub action_taken: Option<String>,
    pub recorded_by: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MedicalRow {
    pub id: i64,
    pub student_id: i64,
    pub record_type: String,
    pub title: String,
    pub details: Option<String>,
    pub recorded_on: NaiveDate,
    pub recorded_by: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IncidentInput {
    pub student_id: i64,
    pub incident_date: NaiveDate,
    pub kind: String,
    pub category: Option<String>,
    pub description: String,
    pub action_taken: Option<String>,
    pub recorded_by: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MedicalInput {
    pub student_id: i64,
    pub record_type: String,
    pub title: String,
    pub details: Option<String>,
    pub recorded_on: NaiveDate,
    pub recorded_by: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------

pub const ALLOWED_INCIDENT_KINDS: &[&str] = &["minor", "major", "positive"];
pub const ALLOWED_MEDICAL_TYPES: &[&str] =
    &["immunization", "allergy", "medication", "condition", "note"];

pub fn validate_incident(description: &str, kind: &str) -> Result<(), String> {
    if description.trim().is_empty() {
        return Err("Description is required".to_string());
    }
    if !ALLOWED_INCIDENT_KINDS.contains(&kind.trim()) {
        return Err("Kind must be minor, major, or positive".to_string());
    }
    Ok(())
}

pub fn validate_medical(title: &str, record_type: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("Title is required".to_string());
    }
    if !ALLOWED_MEDICAL_TYPES.contains(&record_type.trim()) {
        return Err(
            "Record type must be immunization, allergy, medication, condition, or note".to_string(),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct IncidentSummary {
    pub minor: i64,
    pub major: i64,
    pub positive: i64,
    pub total: i64,
}

// ---------------------------------------------------------------------------
// Incidents CRUD
// ---------------------------------------------------------------------------

pub async fn find_incident(pool: &PgPool, id: i64) -> Result<Option<IncidentRow>, AppError> {
    let row = sqlx::query_as::<_, IncidentRow>(
        "SELECT id, student_id, incident_date, kind, category, description, action_taken, recorded_by, created_at
         FROM behavior_incidents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_incidents(pool: &PgPool, student_id: i64) -> Result<Vec<IncidentRow>, AppError> {
    let rows = sqlx::query_as::<_, IncidentRow>(
        "SELECT id, student_id, incident_date, kind, category, description, action_taken, recorded_by, created_at
         FROM behavior_incidents WHERE student_id = $1 ORDER BY incident_date DESC, id DESC",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_incidents_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<Vec<IncidentRow>, AppError> {
    list_incidents(pool, student_id).await
}

pub async fn create_incident(
    pool: &PgPool,
    input: &IncidentInput,
) -> Result<IncidentRow, AppError> {
    if let Err(msg) = validate_incident(&input.description, &input.kind) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, IncidentRow>(
        "INSERT INTO behavior_incidents (student_id, incident_date, kind, category, description, action_taken, recorded_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, student_id, incident_date, kind, category, description, action_taken, recorded_by, created_at",
    )
    .bind(input.student_id)
    .bind(input.incident_date)
    .bind(input.kind.trim())
    .bind(input.category.clone())
    .bind(input.description.trim())
    .bind(input.action_taken.clone())
    .bind(input.recorded_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_incident_fields(
    pool: &PgPool,
    student_id: i64,
    incident_date: NaiveDate,
    kind: &str,
    category: Option<String>,
    description: &str,
    action_taken: Option<String>,
    recorded_by: Option<i64>,
) -> Result<IncidentRow, AppError> {
    let input = IncidentInput {
        student_id,
        incident_date,
        kind: kind.to_string(),
        category,
        description: description.to_string(),
        action_taken,
        recorded_by,
    };
    create_incident(pool, &input).await
}

pub async fn delete_incident(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM behavior_incidents WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_incident(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_incident(pool, id).await
}

// ---------------------------------------------------------------------------
// Medical CRUD
// ---------------------------------------------------------------------------

pub async fn find_medical(pool: &PgPool, id: i64) -> Result<Option<MedicalRow>, AppError> {
    let row = sqlx::query_as::<_, MedicalRow>(
        "SELECT id, student_id, record_type, title, details, recorded_on, recorded_by, created_at
         FROM medical_records WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_medical(pool: &PgPool, student_id: i64) -> Result<Vec<MedicalRow>, AppError> {
    let rows = sqlx::query_as::<_, MedicalRow>(
        "SELECT id, student_id, record_type, title, details, recorded_on, recorded_by, created_at
         FROM medical_records WHERE student_id = $1 ORDER BY recorded_on DESC, id DESC",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_medical_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<Vec<MedicalRow>, AppError> {
    list_medical(pool, student_id).await
}

pub async fn list_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<(Vec<IncidentRow>, Vec<MedicalRow>), AppError> {
    let incidents = list_incidents(pool, student_id).await?;
    let medical = list_medical(pool, student_id).await?;
    Ok((incidents, medical))
}

pub async fn create_medical(pool: &PgPool, input: &MedicalInput) -> Result<MedicalRow, AppError> {
    if let Err(msg) = validate_medical(&input.title, &input.record_type) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, MedicalRow>(
        "INSERT INTO medical_records (student_id, record_type, title, details, recorded_on, recorded_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, student_id, record_type, title, details, recorded_on, recorded_by, created_at",
    )
    .bind(input.student_id)
    .bind(input.record_type.trim())
    .bind(input.title.trim())
    .bind(input.details.clone())
    .bind(input.recorded_on)
    .bind(input.recorded_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_medical_fields(
    pool: &PgPool,
    student_id: i64,
    record_type: &str,
    title: &str,
    details: Option<String>,
    recorded_on: NaiveDate,
    recorded_by: Option<i64>,
) -> Result<MedicalRow, AppError> {
    let input = MedicalInput {
        student_id,
        record_type: record_type.to_string(),
        title: title.to_string(),
        details,
        recorded_on,
        recorded_by,
    };
    create_medical(pool, &input).await
}

pub async fn delete_medical(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM medical_records WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_medical(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_medical(pool, id).await
}

// ---------------------------------------------------------------------------
// Summary query
// ---------------------------------------------------------------------------

pub async fn incident_summary(pool: &PgPool, student_id: i64) -> Result<IncidentSummary, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, COUNT(*)::BIGINT FROM behavior_incidents WHERE student_id = $1 GROUP BY kind",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    let mut summary = IncidentSummary::default();
    for (kind, count) in rows {
        match kind.as_str() {
            "minor" => summary.minor = count,
            "major" => summary.major = count,
            "positive" => summary.positive = count,
            _ => {}
        }
        summary.total += count;
    }
    Ok(summary)
}

pub async fn incident_counts(pool: &PgPool, student_id: i64) -> Result<(i64, i64, i64), AppError> {
    let s = incident_summary(pool, student_id).await?;
    Ok((s.minor, s.major, s.positive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_incident_description() {
        assert!(validate_incident("", "minor").is_err());
        assert!(validate_incident("   ", "minor").is_err());
        assert!(validate_incident("Fighting", "minor").is_ok());
    }

    #[test]
    fn validates_incident_kind() {
        assert!(validate_incident("x", "minor").is_ok());
        assert!(validate_incident("x", "major").is_ok());
        assert!(validate_incident("x", "positive").is_ok());
        assert!(validate_incident("x", "unknown").is_err());
        assert!(validate_incident("x", "").is_err());
    }

    #[test]
    fn validates_medical_title() {
        assert!(validate_medical("", "note").is_err());
        assert!(validate_medical("   ", "note").is_err());
        assert!(validate_medical("Vaccine", "note").is_ok());
    }

    #[test]
    fn validates_medical_type() {
        for t in ["immunization", "allergy", "medication", "condition", "note"] {
            assert!(validate_medical("T", t).is_ok(), "{t}");
        }
        assert!(validate_medical("T", "foo").is_err());
        assert!(validate_medical("T", "").is_err());
    }

    #[test]
    fn summary_default_zero() {
        let s = IncidentSummary::default();
        assert_eq!(s.total, 0);
    }
}
