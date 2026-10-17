use std::collections::HashSet;

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;
use crate::models::student::{create_student, list_students, validate_student, StudentInput};

/// One row of the `audit_log` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub entity: String,
    pub entity_id: Option<i64>,
    pub detail: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Summary returned by [`csv_import_students`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub inserted: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_action<S: AsRef<str>>(action: S) -> Result<(), String> {
    let s = action.as_ref();
    if s.trim().is_empty() {
        return Err("Action must not be empty".to_string());
    }
    if s.len() > 64 {
        return Err("Action must be at most 64 characters".to_string());
    }
    Ok(())
}

pub fn validate_entity<S: AsRef<str>>(entity: S) -> Result<(), String> {
    let s = entity.as_ref();
    if s.trim().is_empty() {
        return Err("Entity must not be empty".to_string());
    }
    if s.len() > 64 {
        return Err("Entity must be at most 64 characters".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Conversion helpers for generic params
// ---------------------------------------------------------------------------

pub trait IntoAuditId {
    fn into_audit_id(self) -> Option<i64>;
}

impl IntoAuditId for i64 {
    fn into_audit_id(self) -> Option<i64> {
        Some(self)
    }
}

impl IntoAuditId for Option<i64> {
    fn into_audit_id(self) -> Option<i64> {
        self
    }
}

pub trait IntoAuditDetail {
    fn into_audit_detail(self) -> Option<String>;
}

impl IntoAuditDetail for Option<&str> {
    fn into_audit_detail(self) -> Option<String> {
        self.map(|s| s.to_string())
    }
}

impl IntoAuditDetail for Option<String> {
    fn into_audit_detail(self) -> Option<String> {
        self
    }
}

impl IntoAuditDetail for &str {
    fn into_audit_detail(self) -> Option<String> {
        Some(self.to_string())
    }
}

impl IntoAuditDetail for String {
    fn into_audit_detail(self) -> Option<String> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn log_action<U, A, En, E, D>(
    pool: &PgPool,
    user_id: U,
    action: A,
    entity: En,
    entity_id: E,
    detail: D,
) -> Result<(), AppError>
where
    U: IntoAuditId,
    A: AsRef<str>,
    En: AsRef<str>,
    E: IntoAuditId,
    D: IntoAuditDetail,
{
    let action = action.as_ref();
    let entity = entity.as_ref();
    validate_action(action).map_err(AppError::BadRequest)?;
    validate_entity(entity).map_err(AppError::BadRequest)?;
    let user_id = user_id.into_audit_id();
    let entity_id = entity_id.into_audit_id();
    let detail = detail.into_audit_detail();
    sqlx::query(
        "INSERT INTO audit_log (user_id, action, entity, entity_id, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity)
    .bind(entity_id)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_recent(pool: &PgPool) -> Result<Vec<AuditRow>, AppError> {
    let rows = sqlx::query_as::<_, AuditRow>(
        "SELECT id, user_id, action, entity, entity_id, detail, created_at FROM audit_log ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_for<E: AsRef<str>>(
    pool: &PgPool,
    entity: E,
    id: i64,
) -> Result<Vec<AuditRow>, AppError> {
    let entity = entity.as_ref();
    let rows = sqlx::query_as::<_, AuditRow>(
        "SELECT id, user_id, action, entity, entity_id, detail, created_at FROM audit_log WHERE entity = $1 AND entity_id = $2 ORDER BY created_at DESC LIMIT 200",
    )
    .bind(entity)
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

pub fn csv_quote(field: &str) -> String {
    // Neutralize spreadsheet formula injection: a cell beginning with =,+,-,@
    // (or a lone tab/CR) is executed as a formula by Excel/Sheets on open.
    // Prefix such fields with a single quote before applying CSV quoting.
    let needs_formula_guard = field
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let guarded;
    let field = if needs_formula_guard {
        guarded = format!("'{field}");
        guarded.as_str()
    } else {
        field
    };
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

// aliases for uncertain helper names
pub fn quote_csv(field: &str) -> String {
    csv_quote(field)
}
pub fn escape_csv(field: &str) -> String {
    csv_quote(field)
}
pub fn csv_escape(field: &str) -> String {
    csv_quote(field)
}
pub fn quote_field(field: &str) -> String {
    csv_quote(field)
}

pub async fn csv_export_students(pool: &PgPool) -> Result<Vec<Vec<String>>, AppError> {
    let students = list_students(pool).await?;
    let mut out: Vec<Vec<String>> = Vec::new();
    out.push(vec![
        "admission_no".to_string(),
        "first_name".to_string(),
        "last_name".to_string(),
        "status".to_string(),
        "enrolled_on".to_string(),
    ]);
    for s in students {
        out.push(vec![
            s.admission_no,
            s.first_name,
            s.last_name,
            s.status,
            s.enrolled_on.to_string(),
        ]);
    }
    Ok(out)
}

fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut current: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    field.push('"');
                    i += 2;
                } else {
                    in_quotes = false;
                    i += 1;
                }
            } else {
                field.push(c);
                i += 1;
            }
        } else {
            match c {
                '"' => {
                    in_quotes = true;
                    field_started = true;
                    i += 1;
                }
                ',' => {
                    current.push(field.clone());
                    field.clear();
                    field_started = false;
                    i += 1;
                }
                '\n' => {
                    current.push(field.clone());
                    field.clear();
                    field_started = false;
                    if !current.iter().all(|f| f.trim().is_empty()) {
                        rows.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                    i += 1;
                }
                '\r' => {
                    i += 1;
                }
                _ => {
                    field.push(c);
                    field_started = true;
                    i += 1;
                }
            }
        }
    }
    if field_started || !field.is_empty() || !current.is_empty() {
        current.push(field);
        if !current.iter().all(|f| f.trim().is_empty()) {
            rows.push(current);
        }
    }
    rows
}

pub async fn csv_import_students(
    pool: &PgPool,
    csv_text: &str,
) -> Result<ImportReport, AppError> {
    let mut rows = parse_csv(csv_text);
    // drop header if present
    if !rows.is_empty() {
        let first = rows[0]
            .first()
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        if first == "admission_no" {
            rows.remove(0);
        }
    }

    let mut report = ImportReport {
        inserted: 0,
        skipped: 0,
        errors: Vec::new(),
    };
    let mut seen: HashSet<String> = HashSet::new();

    for (idx, row) in rows.iter().enumerate() {
        let row_no = idx + 1;
        // pad
        let get = |i: usize| -> String {
            row.get(i).cloned().unwrap_or_default()
        };
        let admission_no = get(0).trim().to_string();
        let first_name = get(1).trim().to_string();
        let last_name = get(2).trim().to_string();
        let status_raw = get(3);
        let status = if status_raw.trim().is_empty() {
            "active".to_string()
        } else {
            status_raw.trim().to_string()
        };
        let enrolled_raw = get(4);

        // basic validation
        if let Err(msg) = validate_student(&admission_no, &first_name, &last_name, None, &status) {
            report.errors.push(format!("Row {}: {}", row_no, msg));
            continue;
        }

        let enrolled_on = if enrolled_raw.trim().is_empty() {
            chrono::Local::now().date_naive()
        } else {
            match NaiveDate::parse_from_str(enrolled_raw.trim(), "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => {
                    report
                        .errors
                        .push(format!("Row {}: invalid enrolled_on date", row_no));
                    continue;
                }
            }
        };

        // duplicate within file
        let key = admission_no.to_lowercase();
        if seen.contains(&key) {
            report.skipped += 1;
            continue;
        }
        seen.insert(key);

        let input = StudentInput {
            admission_no: admission_no.clone(),
            first_name,
            last_name,
            date_of_birth: None,
            gender: None,
            guardian_name: None,
            guardian_phone: None,
            guardian_email: None,
            address: None,
            status,
            enrolled_on,
        };

        match create_student(pool, &input).await {
            Ok(_) => {
                report.inserted += 1;
            }
            Err(AppError::Db(sqlx::Error::Database(db))) if db.code().as_deref() == Some("23505") => {
                report.skipped += 1;
            }
            // Record the row that failed and keep going, instead of aborting the
            // whole import with a 500 after some rows already committed.
            Err(e) => {
                report.errors.push(format!("row {}: {}", report.inserted + report.skipped + report.errors.len() + 1, e));
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_action_rules() {
        assert!(validate_action("create").is_ok());
        assert!(validate_action("").is_err());
        assert!(validate_action("   ").is_err());
        let long = "a".repeat(65);
        assert!(validate_action(&long).is_err());
        let ok = "a".repeat(64);
        assert!(validate_action(&ok).is_ok());
    }

    #[test]
    fn validate_entity_rules() {
        assert!(validate_entity("student").is_ok());
        assert!(validate_entity("").is_err());
        assert!(validate_entity("   ").is_err());
        let long = "a".repeat(65);
        assert!(validate_entity(&long).is_err());
    }

    #[test]
    fn csv_quote_rules() {
        assert_eq!(csv_quote("simple"), "simple");
        assert_eq!(csv_quote("a,b"), "\"a,b\"");
        assert_eq!(csv_quote("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_quote("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn parse_csv_handles_quotes() {
        let text = "admission_no,first_name,last_name,status,enrolled_on\nA1,Ada,Lovelace,active,2024-09-01\n\"A,2\",Grace,Hopper,active,2024-09-02";
        let rows = parse_csv(text);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2][0], "A,2");
    }
}
