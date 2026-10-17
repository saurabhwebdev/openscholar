use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AnalyticsRow {
    pub student_id: i64,
    pub full_name: String,
    pub admission_no: String,
    pub fee_balance: i64,
    pub attendance_rate: Option<f64>,
    pub avg_grade: Option<f64>,
    #[sqlx(default)]
    pub fee_risk: bool,
    #[sqlx(default)]
    pub attendance_risk: bool,
    #[sqlx(default)]
    pub grade_risk: bool,
    pub reasons: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AttendanceTrendRow {
    pub date: chrono::NaiveDate,
    pub present: i64,
    pub absent: i64,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct FeeSummary {
    pub total_billed: i64,
    pub total_paid: i64,
    pub outstanding: i64,
    // aliases
    pub billed: i64,
    pub paid: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GradeSummary {
    pub class_id: i64,
    pub class_name: String,
    pub avg: f64,
    #[sqlx(default)]
    pub average: f64,
}

#[derive(Debug, Clone)]
pub struct EnrollmentCounts {
    pub students: i64,
    pub staff: i64,
    pub classes: i64,
    pub active_students: i64,
    pub active_staff: i64,
    pub active_classes: i64,
}

// ---------------------------------------------------------------------------
// Pure helper
// ---------------------------------------------------------------------------

pub fn risk_reasons(fee_balance_gt_zero: bool, attendance_lt_80: bool, avg_grade_lt_50: bool) -> Vec<String> {
    let mut out = Vec::new();
    if fee_balance_gt_zero {
        out.push("Fee balance outstanding".to_string());
    }
    if attendance_lt_80 {
        out.push("Low attendance".to_string());
    }
    if avg_grade_lt_50 {
        out.push("Low grades".to_string());
    }
    out
}

// aliases
pub fn risk_reasons_for(fee: bool, attendance: bool, grade: bool) -> Vec<String> {
    risk_reasons(fee, attendance, grade)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn at_risk_students(pool: &PgPool) -> Result<Vec<AnalyticsRow>, AppError> {
    let rows = sqlx::query_as::<_, (i64, String, String, i64, Option<f64>, Option<f64>)>(
        "SELECT s.id AS student_id,
                s.first_name || ' ' || s.last_name AS full_name,
                s.admission_no,
                (COALESCE((SELECT SUM(amount_cents) FROM invoices WHERE student_id = s.id), 0)
                 - COALESCE((SELECT SUM(p.amount_cents) FROM payments p JOIN invoices i ON i.id = p.invoice_id WHERE i.student_id = s.id), 0))::BIGINT AS fee_balance,
                (SELECT CASE WHEN COUNT(*) > 0
                            THEN COUNT(*) FILTER (WHERE status = 'present')::FLOAT8 * 100.0 / COUNT(*)::FLOAT8
                            ELSE 100.0 END
                 FROM attendance WHERE student_id = s.id AND date >= CURRENT_DATE - INTERVAL '30 days') AS attendance_rate,
                (SELECT AVG(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0) * 100.0)
                 FROM grades g JOIN assessments a ON a.id = g.assessment_id
                 WHERE g.student_id = s.id AND g.score IS NOT NULL) AS avg_grade
         FROM students s
         WHERE s.status = 'active'",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for (student_id, full_name, admission_no, fee_balance, attendance_rate, avg_grade) in rows {
        let fee_risk = fee_balance > 0;
        let attendance_risk = attendance_rate.map(|r| r < 80.0).unwrap_or(false);
        let grade_risk = avg_grade.map(|g| g < 50.0).unwrap_or(false);
        if !(fee_risk || attendance_risk || grade_risk) {
            continue;
        }
        let reasons = risk_reasons(fee_risk, attendance_risk, grade_risk).join(", ");
        out.push(AnalyticsRow {
            student_id,
            full_name,
            admission_no,
            fee_balance,
            attendance_rate,
            avg_grade,
            fee_risk,
            attendance_risk,
            grade_risk,
            reasons,
        });
    }
    Ok(out)
}

// aliases
pub async fn at_risk(pool: &PgPool) -> Result<Vec<AnalyticsRow>, AppError> {
    at_risk_students(pool).await
}
pub async fn list_at_risk(pool: &PgPool) -> Result<Vec<AnalyticsRow>, AppError> {
    at_risk_students(pool).await
}

pub async fn attendance_trend(pool: &PgPool) -> Result<Vec<AttendanceTrendRow>, AppError> {
    let rows = sqlx::query_as::<_, AttendanceTrendRow>(
        "SELECT date,
                COUNT(*) FILTER (WHERE status = 'present')::BIGINT AS present,
                COUNT(*) FILTER (WHERE status = 'absent')::BIGINT AS absent,
                COUNT(*)::BIGINT AS total
         FROM attendance
         WHERE date >= CURRENT_DATE - INTERVAL '13 days'
         GROUP BY date
         ORDER BY date",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn attendance_trend_14(pool: &PgPool) -> Result<Vec<AttendanceTrendRow>, AppError> {
    attendance_trend(pool).await
}

pub async fn fee_summary(pool: &PgPool) -> Result<FeeSummary, AppError> {
    let billed: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_cents), 0)::BIGINT FROM invoices",
    )
    .fetch_one(pool)
    .await?;
    let paid: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_cents), 0)::BIGINT FROM payments",
    )
    .fetch_one(pool)
    .await?;
    let outstanding = billed - paid;
    Ok(FeeSummary {
        total_billed: billed,
        total_paid: paid,
        outstanding,
        billed,
        paid,
    })
}

pub async fn fee_totals(pool: &PgPool) -> Result<(i64, i64, i64), AppError> {
    let s = fee_summary(pool).await?;
    Ok((s.total_billed, s.total_paid, s.outstanding))
}

pub async fn grade_summary(pool: &PgPool) -> Result<Vec<GradeSummary>, AppError> {
    let rows = sqlx::query_as::<_, (i64, String, Option<f64>)>(
        "SELECT c.id AS class_id, c.name AS class_name,
                AVG(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0) * 100.0) AS avg
         FROM classes c
         LEFT JOIN assessments a ON a.class_id = c.id
         LEFT JOIN grades g ON g.assessment_id = a.id AND g.score IS NOT NULL
         GROUP BY c.id, c.name
         ORDER BY c.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(class_id, class_name, avg)| {
            let v = avg.unwrap_or(0.0);
            GradeSummary {
                class_id,
                class_name,
                avg: v,
                average: v,
            }
        })
        .collect())
}

pub async fn grade_summary_rows(pool: &PgPool) -> Result<Vec<GradeSummary>, AppError> {
    grade_summary(pool).await
}

pub async fn enrollment_counts(pool: &PgPool) -> Result<EnrollmentCounts, AppError> {
    let students: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM students WHERE status = 'active'")
        .fetch_one(pool)
        .await?;
    let staff: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM staff WHERE status = 'active'")
        .fetch_one(pool)
        .await?;
    let classes: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM classes")
        .fetch_one(pool)
        .await?;
    Ok(EnrollmentCounts {
        students,
        staff,
        classes,
        active_students: students,
        active_staff: staff,
        active_classes: classes,
    })
}

pub async fn enrollment_totals(pool: &PgPool) -> Result<(i64, i64, i64), AppError> {
    let c = enrollment_counts(pool).await?;
    Ok((c.students, c.staff, c.classes))
}