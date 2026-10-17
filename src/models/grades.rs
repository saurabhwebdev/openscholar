use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssessmentRow {
    pub id: i64,
    pub class_id: i64,
    pub name: String,
    pub kind: String,
    pub max_score: f64,
    pub weight: f64,
    pub date: NaiveDate,
}

/// One assessment with its class-wide average and graded-student count.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssessmentView {
    pub id: i64,
    pub class_id: i64,
    pub name: String,
    pub kind: String,
    pub max_score: f64,
    pub weight: f64,
    pub date: NaiveDate,
    pub avg: Option<f64>,
    pub graded_count: i64,
}

/// Assessment header plus its class name (for the score-grid page).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssessmentHeader {
    pub id: i64,
    pub class_id: i64,
    pub class_name: String,
    pub name: String,
    pub kind: String,
    pub max_score: f64,
    pub weight: f64,
    pub date: NaiveDate,
}

/// One score-grid line: an enrolled student plus their score (if graded).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GradeEntry {
    pub student_id: i64,
    pub full_name: String,
    pub score: Option<f64>,
}

/// Full detail read model: header + one entry per enrolled student.
#[derive(Debug, Clone)]
pub struct AssessmentDetail {
    pub assessment: AssessmentRow,
    pub class_name: String,
    pub entries: Vec<GradeEntry>,
}

/// One per-student line of the class overview table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StudentOverview {
    pub student_id: i64,
    pub full_name: String,
    pub percent: Option<f64>,
    pub graded: i64,
}

/// Class `<select>` option for the grades picker.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassOption {
    pub id: i64,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// `name` non-empty; `kind` in {exam, quiz, assignment, project};
/// `max_score` > 0; `weight` >= 0.
pub fn validate_assessment(
    name: &str,
    kind: &str,
    max_score: f64,
    weight: f64,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Assessment name is required".to_string());
    }
    if !matches!(kind, "exam" | "quiz" | "assignment" | "project") {
        return Err("Kind must be exam, quiz, assignment, or project".to_string());
    }
    if !max_score.is_finite() || max_score <= 0.0 {
        return Err("Max score must be greater than zero".to_string());
    }
    if !weight.is_finite() || weight < 0.0 {
        return Err("Weight must be zero or greater".to_string());
    }
    Ok(())
}

/// `None` (cleared cell) is always ok; otherwise `0 <= score <= max`.
pub fn validate_score(score: Option<f64>, max: f64) -> Result<(), String> {
    match score {
        None => Ok(()),
        Some(s) => {
            if !s.is_finite() {
                return Err("Score must be a number".to_string());
            }
            if s < 0.0 || s > max {
                return Err(format!("Score must be between 0 and {max}"));
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only; every NUMERIC is cast to FLOAT8)
// ---------------------------------------------------------------------------

/// Classes of the current school year, ordered by name (picker source).
pub async fn list_class_options(pool: &PgPool) -> Result<Vec<ClassOption>, AppError> {
    let rows = sqlx::query_as::<_, ClassOption>(
        "SELECT c.id, c.name
          FROM classes c
          WHERE c.school_year_id = (SELECT id FROM school_years WHERE is_current LIMIT 1)
          ORDER BY c.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Assessments of a class with per-assessment average and graded count.
pub async fn assessments_for_class(
    pool: &PgPool,
    class_id: i64,
) -> Result<Vec<AssessmentView>, AppError> {
    let rows = sqlx::query_as::<_, AssessmentView>(
        "SELECT a.id, a.class_id, a.name, a.kind,
                a.max_score::FLOAT8 AS max_score,
                a.weight::FLOAT8 AS weight,
                a.date,
                (AVG(g.score))::FLOAT8 AS avg,
                COUNT(g.score) AS graded_count
          FROM assessments a
          LEFT JOIN grades g ON g.assessment_id = a.id
          WHERE a.class_id = $1
          GROUP BY a.id
          ORDER BY a.date, a.id",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_assessment(
    pool: &PgPool,
    class_id: i64,
    name: &str,
    kind: &str,
    max_score: f64,
    weight: f64,
    date: NaiveDate,
) -> Result<AssessmentRow, AppError> {
    let row = sqlx::query_as::<_, AssessmentRow>(
        "INSERT INTO assessments (class_id, name, kind, max_score, weight, date)
          VALUES ($1, $2, $3, $4, $5, $6)
          RETURNING id, class_id, name, kind,
                    max_score::FLOAT8 AS max_score,
                    weight::FLOAT8 AS weight,
                    date",
    )
    .bind(class_id)
    .bind(name.trim())
    .bind(kind)
    .bind(max_score)
    .bind(weight)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Header (with class name) plus one entry per enrolled student.
/// Students without a grade row get `score: None`.
pub async fn assessment_detail(pool: &PgPool, id: i64) -> Result<AssessmentDetail, AppError> {
    let header = sqlx::query_as::<_, AssessmentHeader>(
        "SELECT a.id, a.class_id, c.name AS class_name, a.name, a.kind,
                a.max_score::FLOAT8 AS max_score,
                a.weight::FLOAT8 AS weight,
                a.date
          FROM assessments a
          JOIN classes c ON c.id = a.class_id
          WHERE a.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let entries = sqlx::query_as::<_, GradeEntry>(
        "SELECT s.id AS student_id,
                (s.first_name || ' ' || s.last_name) AS full_name,
                g.score::FLOAT8 AS score
          FROM enrollments e
          JOIN students s ON s.id = e.student_id
          LEFT JOIN grades g ON g.assessment_id = $1 AND g.student_id = s.id
          WHERE e.class_id = $2
          ORDER BY s.last_name, s.first_name",
    )
    .bind(id)
    .bind(header.class_id)
    .fetch_all(pool)
    .await?;
    Ok(AssessmentDetail {
        assessment: AssessmentRow {
            id: header.id,
            class_id: header.class_id,
            name: header.name.clone(),
            kind: header.kind.clone(),
            max_score: header.max_score,
            weight: header.weight,
            date: header.date,
        },
        class_name: header.class_name.clone(),
        entries,
    })
}

/// Upsert each score; `None` deletes the grade row (allows clearing a score).
pub async fn save_scores(
    pool: &PgPool,
    assessment_id: i64,
    scores: &[(i64, Option<f64>)],
) -> Result<(), AppError> {
    // Whole grid saves all-or-nothing: a mid-loop failure must not leave some
    // students' scores saved and others lost.
    let mut tx = pool.begin().await?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM assessments WHERE id = $1")
        .bind(assessment_id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    for (student_id, score) in scores {
        match score {
            None => {
                sqlx::query("DELETE FROM grades WHERE assessment_id = $1 AND student_id = $2")
                    .bind(assessment_id)
                    .bind(student_id)
                    .execute(&mut *tx)
                    .await?;
            }
            Some(s) => {
                sqlx::query(
                    "INSERT INTO grades (assessment_id, student_id, score)
                      VALUES ($1, $2, $3)
                      ON CONFLICT (assessment_id, student_id)
                      DO UPDATE SET score = EXCLUDED.score",
                )
                .bind(assessment_id)
                .bind(student_id)
                .bind(s)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Delete an assessment. Its grades cascade via FK.
/// Returns the owning class id for the redirect; 404 when 0 rows.
pub async fn delete_assessment(pool: &PgPool, id: i64) -> Result<i64, AppError> {
    let class: Option<i64> =
        sqlx::query_scalar("DELETE FROM assessments WHERE id = $1 RETURNING class_id")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    class.ok_or(AppError::NotFound)
}

/// Per-student weighted percent over graded assessments plus graded count.
/// Percent is `100 * SUM(score/max * weight) / SUM(weight of graded work)`,
/// `NULL` when the student has no grades yet.
pub async fn class_overview(
    pool: &PgPool,
    class_id: i64,
) -> Result<Vec<StudentOverview>, AppError> {
    let rows = sqlx::query_as::<_, StudentOverview>(
        "SELECT s.id AS student_id,
                (s.first_name || ' ' || s.last_name) AS full_name,
                (CASE
                    WHEN SUM(CASE WHEN g.score IS NOT NULL THEN a.weight::FLOAT8 ELSE 0 END) > 0
                        THEN 100 * SUM(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0) * a.weight::FLOAT8)
                             / SUM(CASE WHEN g.score IS NOT NULL THEN a.weight::FLOAT8 ELSE 0 END)
                    WHEN COUNT(g.score) > 0
                        THEN 100 * AVG(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0))
                    ELSE NULL
                 END)::FLOAT8 AS percent,
                COUNT(g.score) AS graded
          FROM enrollments e
          JOIN students s ON s.id = e.student_id
          LEFT JOIN assessments a ON a.class_id = e.class_id
          LEFT JOIN grades g ON g.assessment_id = a.id AND g.student_id = s.id
          WHERE e.class_id = $1
          GROUP BY s.id, s.first_name, s.last_name
          ORDER BY s.last_name, s.first_name",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_assessment() {
        assert!(validate_assessment("Midterm", "exam", 100.0, 1.0).is_ok());
        assert!(validate_assessment("Quiz 1", "quiz", 20.0, 0.5).is_ok());
        assert!(validate_assessment("Essay", "assignment", 50.0, 0.0).is_ok());
        assert!(validate_assessment("Fair", "project", 10.0, 2.0).is_ok());
    }

    #[test]
    fn rejects_blank_name() {
        assert!(validate_assessment("", "exam", 100.0, 1.0).is_err());
        assert!(validate_assessment("   ", "exam", 100.0, 1.0).is_err());
    }

    #[test]
    fn rejects_unknown_kind() {
        assert!(validate_assessment("Midterm", "test", 100.0, 1.0).is_err());
        assert!(validate_assessment("Midterm", "", 100.0, 1.0).is_err());
        assert!(validate_assessment("Midterm", "EXAM", 100.0, 1.0).is_err());
    }

    #[test]
    fn rejects_non_positive_max_score() {
        assert!(validate_assessment("Midterm", "exam", 0.0, 1.0).is_err());
        assert!(validate_assessment("Midterm", "exam", -10.0, 1.0).is_err());
        assert!(validate_assessment("Midterm", "exam", f64::NAN, 1.0).is_err());
        assert!(validate_assessment("Midterm", "exam", f64::INFINITY, 1.0).is_err());
    }

    #[test]
    fn rejects_negative_weight() {
        assert!(validate_assessment("Midterm", "exam", 100.0, -1.0).is_err());
        assert!(validate_assessment("Midterm", "exam", 100.0, f64::NAN).is_err());
    }

    #[test]
    fn accepts_none_score_as_cleared() {
        assert!(validate_score(None, 100.0).is_ok());
    }

    #[test]
    fn accepts_score_within_range() {
        assert!(validate_score(Some(0.0), 100.0).is_ok());
        assert!(validate_score(Some(75.5), 100.0).is_ok());
        assert!(validate_score(Some(100.0), 100.0).is_ok());
    }

    #[test]
    fn rejects_negative_score() {
        assert!(validate_score(Some(-0.5), 100.0).is_err());
    }

    #[test]
    fn rejects_score_above_max() {
        assert!(validate_score(Some(100.5), 100.0).is_err());
        assert!(validate_score(Some(21.0), 20.0).is_err());
    }
}
