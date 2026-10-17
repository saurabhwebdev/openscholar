use sqlx::PgPool;

use crate::error::AppError;

/// Demo seed identifiers. Kept as constants so reruns and tests agree.
pub const DEMO_YEAR_NAME: &str = "Demo 2025-26";
pub const DEMO_STAFF_NOS: [&str; 2] = ["T900", "T901"];
pub const DEMO_STUDENT_NOS: [&str; 3] = ["S900", "S901", "S902"];
pub const DEMO_CLASS_NAME: &str = "Demo Class A";

/// Inserts a small demo dataset for local exploration.
///
/// - Returns `Ok(false)` without touching anything when `students` is
///   non-empty (never disturb real data).
/// - The demo year is always `is_current = FALSE` (never disturb the real
///   current year).
/// - Every insert uses `ON CONFLICT DO NOTHING` so reruns are safe.
/// - Returns `true` when the demo year row was inserted by this call.
pub async fn seed_demo(pool: &PgPool) -> Result<bool, AppError> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM students")
        .fetch_one(pool)
        .await?;
    if existing > 0 {
        return Ok(false);
    }

    // On a fresh DB (no current year) make the demo year current so the
    // dashboard/classes/grades/portal are populated; if a real current year
    // already exists, stay non-intrusive and leave it alone.
    let year_inserted = sqlx::query(
        "INSERT INTO school_years (name, start_date, end_date, is_current)
         VALUES ($1, '2025-09-01', '2026-06-30',
                 NOT EXISTS (SELECT 1 FROM school_years WHERE is_current))
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_YEAR_NAME)
    .execute(pool)
    .await?
    .rows_affected()
        > 0;

    for (i, no) in DEMO_STAFF_NOS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO staff (employee_no, first_name, last_name)
             VALUES ($1, 'Demo', $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(no)
        .bind(format!("Teacher {}", i + 1))
        .execute(pool)
        .await?;
    }

    for (i, no) in DEMO_STUDENT_NOS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO students (admission_no, first_name, last_name)
             VALUES ($1, 'Demo', $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(no)
        .bind(format!("Student {}", i + 1))
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO classes (school_year_id, name, grade_level, room, teacher_id)
         SELECT y.id, $2, 'Grade 1', 'Room D1',
                (SELECT id FROM staff WHERE employee_no = $3)
         FROM school_years y WHERE y.name = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_YEAR_NAME)
    .bind(DEMO_CLASS_NAME)
    .bind(DEMO_STAFF_NOS[0])
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO enrollments (student_id, class_id)
         SELECT s.id, c.id
         FROM students s
         CROSS JOIN classes c
         JOIN school_years y ON y.id = c.school_year_id
         WHERE s.admission_no = ANY($1)
           AND y.name = $2 AND c.name = $3
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_STUDENT_NOS.to_vec())
    .bind(DEMO_YEAR_NAME)
    .bind(DEMO_CLASS_NAME)
    .execute(pool)
    .await?;

    // A demo assessment + grades so the Grades page isn't empty.
    sqlx::query(
        "INSERT INTO assessments (class_id, name, kind, max_score, weight, date)
         SELECT c.id, 'Demo Quiz', 'quiz', 20, 1, '2025-10-01'
         FROM classes c JOIN school_years y ON y.id = c.school_year_id
         WHERE y.name = $1 AND c.name = $2
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_YEAR_NAME)
    .bind(DEMO_CLASS_NAME)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO grades (assessment_id, student_id, score)
         SELECT a.id, s.id, 16
         FROM assessments a
         JOIN classes c ON c.id = a.class_id
         JOIN school_years y ON y.id = c.school_year_id
         JOIN students s ON s.admission_no = ANY($1)
         WHERE y.name = $2 AND c.name = $3 AND a.name = 'Demo Quiz'
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_STUDENT_NOS.to_vec())
    .bind(DEMO_YEAR_NAME)
    .bind(DEMO_CLASS_NAME)
    .execute(pool)
    .await?;

    // A demo invoice so the Fees page isn't empty.
    sqlx::query(
        "INSERT INTO invoices (student_id, title, amount_cents, due_date)
         SELECT s.id, 'Term 1 Tuition', 50000, '2025-10-15'
         FROM students s WHERE s.admission_no = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(DEMO_STUDENT_NOS[0])
    .execute(pool)
    .await?;

    Ok(year_inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_identifiers_match_spec() {
        assert_eq!(DEMO_YEAR_NAME, "Demo 2025-26");
        assert_eq!(DEMO_STAFF_NOS, ["T900", "T901"]);
        assert_eq!(DEMO_STUDENT_NOS, ["S900", "S901", "S902"]);
        assert!(!DEMO_CLASS_NAME.is_empty());
    }

    #[test]
    fn demo_codes_are_namespaced_away_from_real_data() {
        for no in DEMO_STAFF_NOS {
            assert!(no.starts_with('T'));
        }
        for no in DEMO_STUDENT_NOS {
            assert!(no.starts_with('S'));
        }
        assert!(DEMO_YEAR_NAME.starts_with("Demo "));
    }
}
