use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

/// Row mirroring the `staff` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StaffRow {
    pub id: i64,
    pub employee_no: String,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub hired_on: Option<NaiveDate>,
    pub status: String,
}

/// Presentation-ready staff with preformatted dates and display fallbacks
/// so Askama templates never touch `Option` directly.
#[derive(Debug, Clone)]
pub struct StaffView {
    pub id: i64,
    pub employee_no: String,
    pub first_name: String,
    pub last_name: String,
    pub full_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub email_display: String,
    pub phone_display: String,
    pub job_title_display: String,
    pub department_display: String,
    pub hired_on: Option<NaiveDate>,
    pub hired_on_display: String,
    pub hired_on_input: String,
    pub status: String,
    pub is_active: bool,
}

fn display(value: &Option<String>) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => v.clone(),
        _ => "—".to_string(),
    }
}

impl From<StaffRow> for StaffView {
    fn from(row: StaffRow) -> Self {
        Self {
            id: row.id,
            employee_no: row.employee_no.clone(),
            first_name: row.first_name.clone(),
            last_name: row.last_name.clone(),
            full_name: format!("{} {}", row.first_name, row.last_name),
            email_display: display(&row.email),
            phone_display: display(&row.phone),
            job_title_display: display(&row.job_title),
            department_display: display(&row.department),
            email: row.email.clone(),
            phone: row.phone.clone(),
            job_title: row.job_title.clone(),
            department: row.department.clone(),
            hired_on_display: row
                .hired_on
                .map(|d| d.format("%-d %B %Y").to_string())
                .unwrap_or_else(|| "—".to_string()),
            hired_on_input: row
                .hired_on
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            hired_on: row.hired_on,
            is_active: row.status == "active",
            status: row.status.clone(),
            // move the owned Strings last via clone above; keep struct tidy
        }
    }
}

/// Pure validation shared by the create and update paths.
pub fn validate_staff(
    first_name: &str,
    last_name: &str,
    employee_no: &str,
    status: &str,
) -> Result<(), AppError> {
    if first_name.trim().is_empty() {
        return Err(AppError::BadRequest("First name is required".to_string()));
    }
    if last_name.trim().is_empty() {
        return Err(AppError::BadRequest("Last name is required".to_string()));
    }
    if employee_no.trim().is_empty() {
        return Err(AppError::BadRequest("Employee number is required".to_string()));
    }
    if status != "active" && status != "inactive" {
        return Err(AppError::BadRequest(
            "Status must be active or inactive".to_string(),
        ));
    }
    Ok(())
}

/// Trimmed, validated input for [`create_staff`] / [`update_staff`].
#[derive(Debug, Clone)]
pub struct StaffInput {
    pub employee_no: String,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub hired_on: Option<NaiveDate>,
    pub status: String,
}

pub async fn find_staff(pool: &PgPool, id: i64) -> Result<Option<StaffRow>, AppError> {
    let row = sqlx::query_as::<_, StaffRow>(
        "SELECT id, employee_no, first_name, last_name, email, phone, job_title, \
         department, hired_on, status FROM staff WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_staff(pool: &PgPool) -> Result<Vec<StaffRow>, AppError> {
    let rows = sqlx::query_as::<_, StaffRow>(
        "SELECT id, employee_no, first_name, last_name, email, phone, job_title, \
         department, hired_on, status FROM staff \
         ORDER BY last_name, first_name LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn search_staff(pool: &PgPool, q: &str) -> Result<Vec<StaffRow>, AppError> {
    let pattern = format!("%{}%", q.trim());
    let rows = sqlx::query_as::<_, StaffRow>(
        "SELECT id, employee_no, first_name, last_name, email, phone, job_title, \
         department, hired_on, status FROM staff \
         WHERE first_name ILIKE $1 OR last_name ILIKE $1 OR employee_no ILIKE $1 \
         OR job_title ILIKE $1 OR department ILIKE $1 \
         ORDER BY last_name, first_name LIMIT 200",
    )
    .bind(pattern)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_staff(pool: &PgPool, input: &StaffInput) -> Result<StaffRow, AppError> {
    validate_staff(
        &input.first_name,
        &input.last_name,
        &input.employee_no,
        &input.status,
    )?;
    let row = sqlx::query_as::<_, StaffRow>(
        "INSERT INTO staff (employee_no, first_name, last_name, email, phone, \
         job_title, department, hired_on, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING id, employee_no, first_name, last_name, email, phone, \
         job_title, department, hired_on, status",
    )
    .bind(&input.employee_no)
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.job_title)
    .bind(&input.department)
    .bind(input.hired_on)
    .bind(&input.status)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_staff(
    pool: &PgPool,
    id: i64,
    input: &StaffInput,
) -> Result<Option<StaffRow>, AppError> {
    validate_staff(
        &input.first_name,
        &input.last_name,
        &input.employee_no,
        &input.status,
    )?;
    let row = sqlx::query_as::<_, StaffRow>(
        "UPDATE staff SET employee_no = $1, first_name = $2, last_name = $3, \
         email = $4, phone = $5, job_title = $6, department = $7, hired_on = $8, \
         status = $9 WHERE id = $10 \
         RETURNING id, employee_no, first_name, last_name, email, phone, \
         job_title, department, hired_on, status",
    )
    .bind(&input.employee_no)
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.job_title)
    .bind(&input.department)
    .bind(input.hired_on)
    .bind(&input.status)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Classes where this staff member is the assigned teacher.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StaffClassRow {
    pub id: i64,
    pub name: String,
    pub grade_level: Option<String>,
    pub room: Option<String>,
    pub school_year: Option<String>,
}

pub async fn classes_for_teacher(
    pool: &PgPool,
    staff_id: i64,
) -> Result<Vec<StaffClassRow>, AppError> {
    let rows = sqlx::query_as::<_, StaffClassRow>(
        "SELECT c.id, c.name, c.grade_level, c.room, y.name AS school_year \
         FROM classes c JOIN school_years y ON y.id = c.school_year_id \
         WHERE c.teacher_id = $1 ORDER BY y.name, c.name",
    )
    .bind(staff_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Login account whose email matches the staff record, if any.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LinkedUserRow {
    pub email: String,
    pub full_name: String,
    pub role: String,
}

pub async fn find_linked_user(
    pool: &PgPool,
    email: &str,
) -> Result<Option<LinkedUserRow>, AppError> {
    let row = sqlx::query_as::<_, LinkedUserRow>(
        "SELECT email, full_name, role FROM users WHERE lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_parts() -> (String, String, String, String) {
        (
            "Ada".to_string(),
            "Lovelace".to_string(),
            "EMP-001".to_string(),
            "active".to_string(),
        )
    }

    #[test]
    fn accepts_valid_active_staff() {
        let (first, last, no, status) = valid_parts();
        assert!(validate_staff(&first, &last, &no, &status).is_ok());
    }

    #[test]
    fn accepts_valid_inactive_staff() {
        let (first, last, no, _) = valid_parts();
        assert!(validate_staff(&first, &last, &no, "inactive").is_ok());
    }

    #[test]
    fn rejects_empty_first_name() {
        let (_, last, no, status) = valid_parts();
        assert!(matches!(
            validate_staff("", &last, &no, &status),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_whitespace_first_name() {
        let (_, last, no, status) = valid_parts();
        assert!(matches!(
            validate_staff("   ", &last, &no, &status),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_empty_last_name() {
        let (first, _, no, status) = valid_parts();
        assert!(matches!(
            validate_staff(&first, "", &no, &status),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_empty_employee_no() {
        let (first, last, _, status) = valid_parts();
        assert!(matches!(
            validate_staff(&first, &last, "", &status),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_whitespace_employee_no() {
        let (first, last, _, status) = valid_parts();
        assert!(matches!(
            validate_staff(&first, &last, "  ", &status),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_unknown_status() {
        let (first, last, no, _) = valid_parts();
        for bad in ["archived", "Active", "", "retired"] {
            assert!(
                matches!(
                    validate_staff(&first, &last, &no, bad),
                    Err(AppError::BadRequest(_))
                ),
                "status {bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn view_preformats_dates_and_fallbacks() {
        let row = StaffRow {
            id: 1,
            employee_no: "EMP-001".to_string(),
            first_name: "Ada".to_string(),
            last_name: "Lovelace".to_string(),
            email: None,
            phone: Some(" ".to_string()),
            job_title: Some("Teacher".to_string()),
            department: None,
            hired_on: NaiveDate::from_ymd_opt(2024, 1, 12),
            status: "active".to_string(),
        };
        let view = StaffView::from(row);
        assert_eq!(view.full_name, "Ada Lovelace");
        assert!(view.is_active);
        assert_eq!(view.hired_on_input, "2024-01-12");
        assert_eq!(view.hired_on_display, "12 January 2024");
        assert_eq!(view.email_display, "—");
        assert_eq!(view.job_title_display, "Teacher");
    }

    #[test]
    fn view_handles_missing_hire_date_and_inactive() {
        let row = StaffRow {
            id: 2,
            employee_no: "EMP-002".to_string(),
            first_name: "Grace".to_string(),
            last_name: "Hopper".to_string(),
            email: Some("g@example.com".to_string()),
            phone: None,
            job_title: None,
            department: None,
            hired_on: None,
            status: "inactive".to_string(),
        };
        let view = StaffView::from(row);
        assert!(!view.is_active);
        assert_eq!(view.hired_on_input, "");
        assert_eq!(view.hired_on_display, "—");
    }
}
