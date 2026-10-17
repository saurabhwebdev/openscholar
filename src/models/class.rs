use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SchoolYearRow {
    pub id: i64,
    pub name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub is_current: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassRow {
    pub id: i64,
    pub school_year_id: i64,
    pub name: String,
    pub grade_level: Option<String>,
    pub room: Option<String>,
    pub teacher_id: Option<i64>,
    pub capacity: Option<i32>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EnrollmentRow {
    pub id: i64,
    pub student_id: i64,
    pub class_id: i64,
    pub enrolled_on: NaiveDate,
}

/// Joined read model for lists / detail.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassView {
    pub id: i64,
    pub name: String,
    pub grade_level: Option<String>,
    pub room: Option<String>,
    pub teacher_name: Option<String>,
    pub capacity: Option<i32>,
    pub enrolled: i64,
    pub year_name: String,
    pub school_year_id: i64,
    pub teacher_id: Option<i64>,
}

/// One roster line on the class detail page.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RosterEntry {
    pub student_id: i64,
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
    pub enrolled_on: NaiveDate,
}

/// Candidate for the enroll picker.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AvailableStudent {
    pub id: i64,
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
}

/// Teacher `<select>` option.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeacherOption {
    pub id: i64,
    pub full_name: String,
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// `name` and `school_year_id` are required; `capacity` when present must be > 0.
pub fn validate_class(
    name: &str,
    school_year_id: Option<i64>,
    capacity: Option<i32>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Class name is required".to_string());
    }
    if school_year_id.is_none() {
        return Err("School year is required".to_string());
    }
    if let Some(c) = capacity {
        if c <= 0 {
            return Err("Capacity must be greater than zero".to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only)
// ---------------------------------------------------------------------------

pub async fn list_years(pool: &PgPool) -> Result<Vec<SchoolYearRow>, AppError> {
    let rows = sqlx::query_as::<_, SchoolYearRow>(
        "SELECT id, name, start_date, end_date, is_current
         FROM school_years ORDER BY start_date DESC, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn current_year(pool: &PgPool) -> Result<Option<SchoolYearRow>, AppError> {
    let row = sqlx::query_as::<_, SchoolYearRow>(
        "SELECT id, name, start_date, end_date, is_current
         FROM school_years WHERE is_current LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_year(
    pool: &PgPool,
    name: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<SchoolYearRow, AppError> {
    let row = sqlx::query_as::<_, SchoolYearRow>(
        "INSERT INTO school_years (name, start_date, end_date)
         VALUES ($1, $2, $3)
         RETURNING id, name, start_date, end_date, is_current",
    )
    .bind(name.trim())
    .bind(start_date)
    .bind(end_date)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Transaction: clear all flags, then set the chosen year.
pub async fn set_current_year(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE school_years SET is_current = FALSE")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("UPDATE school_years SET is_current = TRUE WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_classes(pool: &PgPool, year_id: i64) -> Result<Vec<ClassView>, AppError> {
    let rows = sqlx::query_as::<_, ClassView>(
        "SELECT c.id, c.name, c.grade_level, c.room,
                (s.first_name || ' ' || s.last_name) AS teacher_name,
                c.capacity,
                (SELECT COUNT(*) FROM enrollments e WHERE e.class_id = c.id) AS enrolled,
                y.name AS year_name, c.school_year_id, c.teacher_id
         FROM classes c
         JOIN school_years y ON y.id = c.school_year_id
         LEFT JOIN staff s ON s.id = c.teacher_id
         WHERE c.school_year_id = $1
         ORDER BY c.name",
    )
    .bind(year_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn class_detail(pool: &PgPool, id: i64) -> Result<ClassView, AppError> {
    let row = sqlx::query_as::<_, ClassView>(
        "SELECT c.id, c.name, c.grade_level, c.room,
                (s.first_name || ' ' || s.last_name) AS teacher_name,
                c.capacity,
                (SELECT COUNT(*) FROM enrollments e WHERE e.class_id = c.id) AS enrolled,
                y.name AS year_name, c.school_year_id, c.teacher_id
         FROM classes c
         JOIN school_years y ON y.id = c.school_year_id
         LEFT JOIN staff s ON s.id = c.teacher_id
         WHERE c.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn class_row(pool: &PgPool, id: i64) -> Result<ClassRow, AppError> {
    let row = sqlx::query_as::<_, ClassRow>(
        "SELECT id, school_year_id, name, grade_level, room, teacher_id, capacity
         FROM classes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_class(
    pool: &PgPool,
    school_year_id: i64,
    name: &str,
    grade_level: Option<&str>,
    room: Option<&str>,
    teacher_id: Option<i64>,
    capacity: Option<i32>,
) -> Result<ClassRow, AppError> {
    let row = sqlx::query_as::<_, ClassRow>(
        "INSERT INTO classes (school_year_id, name, grade_level, room, teacher_id, capacity)
         VALUES ($1, $2, NULLIF(trim($3), ''), NULLIF(trim($4), ''), $5, $6)
         RETURNING id, school_year_id, name, grade_level, room, teacher_id, capacity",
    )
    .bind(school_year_id)
    .bind(name.trim())
    .bind(grade_level.unwrap_or("").trim())
    .bind(room.unwrap_or("").trim())
    .bind(teacher_id)
    .bind(capacity)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_class(
    pool: &PgPool,
    id: i64,
    school_year_id: i64,
    name: &str,
    grade_level: Option<&str>,
    room: Option<&str>,
    teacher_id: Option<i64>,
    capacity: Option<i32>,
) -> Result<ClassRow, AppError> {
    let row = sqlx::query_as::<_, ClassRow>(
        "UPDATE classes
         SET school_year_id = $2, name = $3,
             grade_level = NULLIF(trim($4), ''), room = NULLIF(trim($5), ''),
             teacher_id = $6, capacity = $7
         WHERE id = $1
         RETURNING id, school_year_id, name, grade_level, room, teacher_id, capacity",
    )
    .bind(id)
    .bind(school_year_id)
    .bind(name.trim())
    .bind(grade_level.unwrap_or("").trim())
    .bind(room.unwrap_or("").trim())
    .bind(teacher_id)
    .bind(capacity)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn list_roster(pool: &PgPool, class_id: i64) -> Result<Vec<RosterEntry>, AppError> {
    let rows = sqlx::query_as::<_, RosterEntry>(
        "SELECT s.id AS student_id, s.admission_no, s.first_name, s.last_name, e.enrolled_on
         FROM enrollments e JOIN students s ON s.id = e.student_id
         WHERE e.class_id = $1
         ORDER BY s.last_name, s.first_name",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_teacher_options(pool: &PgPool) -> Result<Vec<TeacherOption>, AppError> {
    let rows = sqlx::query_as::<_, TeacherOption>(
        "SELECT id, (first_name || ' ' || last_name) AS full_name
         FROM staff WHERE status = 'active'
         ORDER BY last_name, first_name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Enroll with friendly errors: duplicates and full classes become
/// `BadRequest` (routes turn these into flash messages, never 500s).
pub async fn enroll(pool: &PgPool, class_id: i64, student_id: i64) -> Result<(), AppError> {
    // Lock the class row so the capacity check and the insert are serialized
    // (no TOCTOU over-enrollment under concurrency).
    let mut tx = pool.begin().await?;
    let class: Option<(Option<i32>, i64)> =
        sqlx::query_as("SELECT capacity, school_year_id FROM classes WHERE id = $1 FOR UPDATE")
            .bind(class_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (capacity, school_year_id) = match class {
        Some(c) => c,
        None => return Err(AppError::NotFound),
    };

    // Student must exist and be active (the picker filters active, but a direct
    // POST could supply any id).
    let student_active: Option<bool> =
        sqlx::query_scalar("SELECT (status = 'active') FROM students WHERE id = $1")
            .bind(student_id)
            .fetch_optional(&mut *tx)
            .await?;
    match student_active {
        None => return Err(AppError::BadRequest("Student not found".to_string())),
        Some(false) => {
            return Err(AppError::BadRequest(
                "Only active students can be enrolled".to_string(),
            ));
        }
        Some(true) => {}
    }

    // Don't enroll into an archived (non-current) year when a current year exists.
    let is_current: bool = sqlx::query_scalar(
        "SELECT COALESCE((SELECT is_current FROM school_years WHERE id = $1), FALSE)",
    )
    .bind(school_year_id)
    .fetch_one(&mut *tx)
    .await?;
    let current_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM school_years WHERE is_current)")
            .fetch_one(&mut *tx)
            .await?;
    if current_exists && !is_current {
        return Err(AppError::BadRequest(
            "This class belongs to a past school year; enroll into a current-year class"
                .to_string(),
        ));
    }

    let already: Option<i64> =
        sqlx::query_scalar("SELECT id FROM enrollments WHERE class_id = $1 AND student_id = $2")
            .bind(class_id)
            .bind(student_id)
            .fetch_optional(&mut *tx)
            .await?;
    if already.is_some() {
        return Err(AppError::BadRequest(
            "Student is already enrolled in this class".to_string(),
        ));
    }
    if let Some(cap) = capacity {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM enrollments WHERE class_id = $1")
            .bind(class_id)
            .fetch_one(&mut *tx)
            .await?;
        if count >= cap as i64 {
            return Err(AppError::BadRequest(
                "Class is full — raise capacity before enrolling".to_string(),
            ));
        }
    }
    let result = sqlx::query("INSERT INTO enrollments (class_id, student_id) VALUES ($1, $2)")
        .bind(class_id)
        .bind(student_id)
        .execute(&mut *tx)
        .await;
    match result {
        Ok(_) => {
            tx.commit().await?;
            Ok(())
        }
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Err(AppError::BadRequest(
            "Student is already enrolled in this class".to_string(),
        )),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23503") => Err(
            AppError::BadRequest("Student or class not found".to_string()),
        ),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn unenroll(pool: &PgPool, class_id: i64, student_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM enrollments WHERE class_id = $1 AND student_id = $2")
        .bind(class_id)
        .bind(student_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a class. Enrollments, attendance, assessments and grades cascade
/// via FKs. Returns the owning year id for the redirect; 404 when 0 rows.
pub async fn delete_class(pool: &PgPool, id: i64) -> Result<i64, AppError> {
    // Refuse to delete a class that carries academic history — the FK cascade
    // would wipe its enrolments, a year of attendance, and all its grades.
    let history: i64 = sqlx::query_scalar(
        "SELECT ((SELECT COUNT(*) FROM enrollments WHERE class_id = $1)
               + (SELECT COUNT(*) FROM attendance WHERE class_id = $1)
               + (SELECT COUNT(*) FROM assessments WHERE class_id = $1))::BIGINT",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if history > 0 {
        return Err(AppError::BadRequest(
            "This class has enrolment, attendance or grade records and cannot be deleted."
                .to_string(),
        ));
    }
    let year: Option<i64> =
        sqlx::query_scalar("DELETE FROM classes WHERE id = $1 RETURNING school_year_id")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    year.ok_or(AppError::NotFound)
}

/// Single school year row (needed for the delete guard's `is_current` check).
pub async fn year_row(pool: &PgPool, id: i64) -> Result<SchoolYearRow, AppError> {
    let row = sqlx::query_as::<_, SchoolYearRow>(
        "SELECT id, name, start_date, end_date, is_current
          FROM school_years WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

/// Number of classes in a year (needed for the delete guard).
pub async fn count_classes_in_year(pool: &PgPool, year_id: i64) -> Result<i64, AppError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM classes WHERE school_year_id = $1")
        .bind(year_id)
        .fetch_one(pool)
        .await?;
    Ok(n)
}

/// Delete a school year. Its classes cascade (DANGEROUS) — callers must pass
/// `can_delete_year` first. 404 when 0 rows.
pub async fn delete_year(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let done = sqlx::query("DELETE FROM school_years WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Active students not yet enrolled in `class_id`, optional ILIKE search.
pub async fn available_students_for(
    pool: &PgPool,
    class_id: i64,
    q: &str,
) -> Result<Vec<AvailableStudent>, AppError> {
    let pattern = format!("%{}%", q.trim());
    let rows = sqlx::query_as::<_, AvailableStudent>(
        "SELECT s.id, s.admission_no, s.first_name, s.last_name
         FROM students s
         WHERE s.status = 'active'
           AND NOT EXISTS (SELECT 1 FROM enrollments e
                           WHERE e.student_id = s.id AND e.class_id = $1)
           AND ($2 = '%%' OR s.first_name ILIKE $2 OR s.last_name ILIKE $2
                OR s.admission_no ILIKE $2
                OR (s.first_name || ' ' || s.last_name) ILIKE $2)
         ORDER BY s.last_name, s.first_name
         LIMIT 100",
    )
    .bind(class_id)
    .bind(pattern)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Delete guard (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Guard for school-year deletion: deleting a year cascades its classes
/// (and everything under them), so refuse the current year and any year
/// that still has classes.
pub fn can_delete_year(is_current: bool, class_count: i64) -> Result<(), &'static str> {
    if is_current {
        return Err("The current school year cannot be deleted");
    }
    if class_count > 0 {
        return Err("This school year still has classes — move or delete them first");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_blank_name() {
        assert!(validate_class("", Some(1), None).is_err());
        assert!(validate_class("   ", Some(1), None).is_err());
    }

    #[test]
    fn rejects_missing_year() {
        assert_eq!(
            validate_class("Grade 1A", None, None),
            Err("School year is required".to_string())
        );
    }

    #[test]
    fn rejects_non_positive_capacity() {
        assert!(validate_class("Grade 1A", Some(1), Some(0)).is_err());
        assert!(validate_class("Grade 1A", Some(1), Some(-5)).is_err());
    }

    #[test]
    fn accepts_valid_class_with_capacity() {
        assert!(validate_class("Grade 1A", Some(1), Some(30)).is_ok());
    }

    #[test]
    fn accepts_valid_class_without_capacity() {
        assert!(validate_class("Grade 1A", Some(2), None).is_ok());
    }

    #[test]
    fn trims_name_before_checking() {
        assert!(validate_class("  Grade 1A  ", Some(1), None).is_ok());
    }

    #[test]
    fn refuses_current_year() {
        assert_eq!(
            can_delete_year(true, 0),
            Err("The current school year cannot be deleted")
        );
    }

    #[test]
    fn refuses_year_with_classes() {
        assert_eq!(
            can_delete_year(false, 3),
            Err("This school year still has classes — move or delete them first")
        );
    }

    #[test]
    fn current_refusal_takes_precedence_over_class_count() {
        assert_eq!(
            can_delete_year(true, 5),
            Err("The current school year cannot be deleted")
        );
    }

    #[test]
    fn allows_archived_empty_year() {
        assert!(can_delete_year(false, 0).is_ok());
    }
}
