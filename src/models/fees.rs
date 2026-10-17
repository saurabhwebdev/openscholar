//! Fees module: invoice/payment rows, validation, and runtime-SQL queries.
//!
//! All SQL here is runtime-checked (`sqlx::query` / `query_as`); no
//! compile-time macros beyond `#[derive(sqlx::FromRow)]`.
//!
//! Money is stored as BIGINT cents. Postgres `SUM(bigint)` returns NUMERIC,
//! so every money aggregate is cast back with `(COALESCE(SUM(...), 0))::BIGINT`.
//! Invoice status is computed in Rust, never stored:
//! `balance <= 0` -> paid; past-due with `balance > 0` -> overdue;
//! `paid > 0` -> partial; otherwise unpaid.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;
use crate::util;

pub const ALLOWED_METHODS: &[&str] = &["cash", "card", "bank_transfer", "online", "other"];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// Raw `invoices` row returned by `create_invoice`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InvoiceRow {
    pub id: i64,
    pub student_id: i64,
    pub title: String,
    pub amount_cents: i64,
    pub due_date: NaiveDate,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<i64>,
}

/// One invoice joined with its student and payment totals.
/// `paid` / `balance` come from `::BIGINT`-cast aggregates.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutstandingRow {
    pub id: i64,
    pub student_id: i64,
    pub title: String,
    pub student_name: String,
    pub admission_no: String,
    pub amount_cents: i64,
    pub paid: i64,
    /// Sum of fines levied on this invoice (added to what is owed).
    pub fines: i64,
    /// `amount_cents + fines - paid`.
    pub balance: i64,
    pub due_date: NaiveDate,
    pub category_id: Option<i64>,
    pub discount_cents: i64,
}

/// Single-invoice detail: same shape as [`OutstandingRow`].
pub type InvoiceDetailRow = OutstandingRow;

/// Raw `payments` row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PaymentRow {
    pub id: i64,
    pub invoice_id: i64,
    pub amount_cents: i64,
    pub method: String,
    pub reference: Option<String>,
    pub paid_at: chrono::DateTime<chrono::Utc>,
    pub recorded_by: Option<i64>,
}

/// Minimal student lookup for the admission-no field.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StudentLookup {
    pub id: i64,
    pub admission_no: String,
    pub first_name: String,
    pub last_name: String,
}

impl StudentLookup {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

/// Per-student fee totals (all `::BIGINT`-cast).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StudentTotalsRaw {
    pub billed: i64,
    pub paid: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudentTotals {
    pub billed: i64,
    pub paid: i64,
    pub balance: i64,
}

// ---------------------------------------------------------------------------
// Template-friendly view
// ---------------------------------------------------------------------------

/// Preformatted invoice for Askama (money via [`util::format_cents`],
/// dates as `YYYY-MM-DD` strings, status as a static pill key).
#[derive(Debug, Clone)]
pub struct InvoiceView {
    pub id: i64,
    pub title: String,
    pub student_name: String,
    pub admission_no: String,
    pub amount: String,
    pub paid: String,
    pub fines: String,
    pub balance: String,
    pub status: &'static str,
    pub due: String,
    pub category_id: Option<i64>,
    pub discount_cents: i64,
}

pub fn to_view(row: &OutstandingRow, today: NaiveDate) -> InvoiceView {
    InvoiceView {
        id: row.id,
        title: row.title.clone(),
        student_name: row.student_name.clone(),
        admission_no: row.admission_no.clone(),
        amount: util::format_cents(row.amount_cents),
        paid: util::format_cents(row.paid),
        fines: util::format_cents(row.fines),
        balance: util::format_cents(row.balance),
        status: invoice_status(row.balance, row.paid, row.due_date, today),
        due: row.due_date.to_string(),
        category_id: row.category_id,
        discount_cents: row.discount_cents,
    }
}

/// Computed status, never stored: settled -> `paid`; still owing and past
/// due -> `overdue`; owing with something paid -> `partial`; else `unpaid`.
pub fn invoice_status(
    balance_cents: i64,
    paid_cents: i64,
    due_date: NaiveDate,
    today: NaiveDate,
) -> &'static str {
    if balance_cents <= 0 {
        "paid"
    } else if due_date < today {
        "overdue"
    } else if paid_cents > 0 {
        "partial"
    } else {
        "unpaid"
    }
}

// ---------------------------------------------------------------------------
// Validation (pure, unit-tested)
// ---------------------------------------------------------------------------

/// `title` must be non-empty; `amount_cents` must be > 0; `due_date` must
/// parse as `YYYY-MM-DD`. Returns the parsed date for convenience.
pub fn validate_invoice(
    title: &str,
    amount_cents: i64,
    due_date: &str,
) -> Result<NaiveDate, String> {
    if title.trim().is_empty() {
        return Err("Invoice title is required".to_string());
    }
    if amount_cents <= 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    NaiveDate::parse_from_str(due_date.trim(), "%Y-%m-%d")
        .map_err(|_| "Due date must be YYYY-MM-DD".to_string())
}

/// Parse a human amount (`"1500"`, `"15.00"`, `"1,234.56"`) into cents.
/// At most two decimal places; must be > 0.
pub fn parse_amount_cents(raw: &str) -> Result<i64, String> {
    let cleaned: String = raw.trim().replace([',', ' ', '_'], "");
    if cleaned.is_empty() {
        return Err("Amount is required".to_string());
    }
    if cleaned.starts_with('-') {
        return Err("Amount must be greater than zero".to_string());
    }
    let (major, minor) = match cleaned.split_once('.') {
        Some((m, f)) => (m, Some(f)),
        None => (cleaned.as_str(), None),
    };
    if major.is_empty() {
        return Err("Amount is not a number".to_string());
    }
    let major_val: i64 = major
        .parse()
        .map_err(|_| "Amount is not a number".to_string())?;
    let minor_val: i64 = match minor {
        None => 0,
        Some(f) => match f.len() {
            0 => 0,
            1 => f
                .parse::<i64>()
                .map_err(|_| "Amount is not a number".to_string())?
                * 10,
            2 => f
                .parse::<i64>()
                .map_err(|_| "Amount is not a number".to_string())?,
            _ => return Err("Amount has too many decimal places".to_string()),
        },
    };
    if major_val < 0 || minor_val < 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    let cents = major_val
        .checked_mul(100)
        .and_then(|v| v.checked_add(minor_val))
        .ok_or_else(|| "Amount is too large".to_string())?;
    if cents <= 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    Ok(cents)
}

/// `amount_cents` must be > 0, `method` must be in the allowed set, and the
/// payment must not exceed the outstanding `balance_cents`.
pub fn validate_payment(
    amount_cents: i64,
    method: &str,
    balance_cents: i64,
) -> Result<(), String> {
    if amount_cents <= 0 {
        return Err("Payment amount must be greater than zero".to_string());
    }
    if !ALLOWED_METHODS.contains(&method.trim()) {
        return Err("Unknown payment method".to_string());
    }
    if amount_cents > balance_cents {
        return Err("Payment exceeds outstanding balance".to_string());
    }
    Ok(())
}

/// Pure delete guard: an invoice may be deleted only when zero payments
/// have been recorded. Routes check this before running the `DELETE`.
pub fn can_delete_invoice(paid_cents: i64) -> bool {
    paid_cents <= 0
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only)
// ---------------------------------------------------------------------------

/// Invoices with `balance > 0`, optional search across student names,
/// admission no, and title. Capped at 200 rows.
pub async fn outstanding_invoices(
    pool: &PgPool,
    q: &str,
) -> Result<Vec<OutstandingRow>, AppError> {
    let rows = sqlx::query_as::<_, OutstandingRow>(
        "SELECT i.id AS id, i.student_id AS student_id, i.title AS title,
                (s.first_name || ' ' || s.last_name) AS student_name,
                s.admission_no AS admission_no,
                i.amount_cents AS amount_cents,
                (COALESCE(SUM(p.amount_cents), 0))::BIGINT AS paid,
                (COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0))::BIGINT AS fines,
                (i.amount_cents + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0) - COALESCE(SUM(p.amount_cents), 0))::BIGINT AS balance,
                i.due_date AS due_date,
                i.category_id AS category_id,
                i.discount_cents AS discount_cents
         FROM invoices i
         JOIN students s ON s.id = i.student_id
         LEFT JOIN payments p ON p.invoice_id = i.id
         WHERE ($1 = ''
                OR s.first_name ILIKE '%' || $1 || '%'
                OR s.last_name ILIKE '%' || $1 || '%'
                OR s.admission_no ILIKE '%' || $1 || '%'
                OR i.title ILIKE '%' || $1 || '%'
                OR (s.first_name || ' ' || s.last_name) ILIKE '%' || $1 || '%')
         GROUP BY i.id, i.student_id, i.title, s.first_name, s.last_name,
                  s.admission_no, i.amount_cents, i.due_date, i.category_id, i.discount_cents
         HAVING (i.amount_cents + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0) - COALESCE(SUM(p.amount_cents), 0)) > 0
         ORDER BY i.due_date ASC, i.id ASC
         LIMIT 200",
    )
    .bind(q.trim())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}


/// Like [`outstanding_invoices`] but optionally restricted to one fee category.
pub async fn outstanding_invoices_filtered(
    pool: &PgPool,
    q: &str,
    category_id: Option<i64>,
) -> Result<Vec<OutstandingRow>, AppError> {
    let rows = sqlx::query_as::<_, OutstandingRow>(
        "SELECT i.id AS id, i.student_id AS student_id, i.title AS title,
                (s.first_name || ' ' || s.last_name) AS student_name,
                s.admission_no AS admission_no,
                i.amount_cents AS amount_cents,
                (COALESCE(SUM(p.amount_cents), 0))::BIGINT AS paid,
                (COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0))::BIGINT AS fines,
                (i.amount_cents + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0) - COALESCE(SUM(p.amount_cents), 0))::BIGINT AS balance,
                i.due_date AS due_date,
                i.category_id AS category_id,
                i.discount_cents AS discount_cents
         FROM invoices i
         JOIN students s ON s.id = i.student_id
         LEFT JOIN payments p ON p.invoice_id = i.id
         WHERE ($1 = ''
                OR s.first_name ILIKE '%' || $1 || '%'
                OR s.last_name ILIKE '%' || $1 || '%'
                OR s.admission_no ILIKE '%' || $1 || '%'
                OR i.title ILIKE '%' || $1 || '%'
                OR (s.first_name || ' ' || s.last_name) ILIKE '%' || $1 || '%')
           AND ($2::BIGINT IS NULL OR i.category_id = $2)
         GROUP BY i.id, i.student_id, i.title, s.first_name, s.last_name,
                  s.admission_no, i.amount_cents, i.due_date, i.category_id, i.discount_cents
         HAVING (i.amount_cents + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0) - COALESCE(SUM(p.amount_cents), 0)) > 0
         ORDER BY i.due_date ASC, i.id ASC
         LIMIT 200",
    )
    .bind(q.trim())
    .bind(category_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find_student_by_admission(
    pool: &PgPool,
    admission_no: &str,
) -> Result<Option<StudentLookup>, AppError> {
    let row = sqlx::query_as::<_, StudentLookup>(
        "SELECT id, admission_no, first_name, last_name
         FROM students WHERE admission_no = $1",
    )
    .bind(admission_no.trim())
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_invoice(
    pool: &PgPool,
    student_id: i64,
    title: &str,
    amount_cents: i64,
    due_date: NaiveDate,
    created_by: Option<i64>,
) -> Result<InvoiceRow, AppError> {
    let row = sqlx::query_as::<_, InvoiceRow>(
        "INSERT INTO invoices (student_id, title, amount_cents, due_date, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, student_id, title, amount_cents, due_date, created_at, created_by",
    )
    .bind(student_id)
    .bind(title.trim())
    .bind(amount_cents)
    .bind(due_date)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// One invoice with payment totals, plus its payments (newest first).
/// Missing invoices surface as [`AppError::NotFound`].
pub async fn invoice_detail(
    pool: &PgPool,
    id: i64,
) -> Result<(InvoiceDetailRow, Vec<PaymentRow>), AppError> {
    let invoice = sqlx::query_as::<_, InvoiceDetailRow>(
        "SELECT i.id AS id, i.student_id AS student_id, i.title AS title,
                (s.first_name || ' ' || s.last_name) AS student_name,
                s.admission_no AS admission_no,
                i.amount_cents AS amount_cents,
                (COALESCE(SUM(p.amount_cents), 0))::BIGINT AS paid,
                (COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0))::BIGINT AS fines,
                (i.amount_cents + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f WHERE f.invoice_id = i.id), 0) - COALESCE(SUM(p.amount_cents), 0))::BIGINT AS balance,
                i.due_date AS due_date,
                i.category_id AS category_id,
                i.discount_cents AS discount_cents
         FROM invoices i
         JOIN students s ON s.id = i.student_id
         LEFT JOIN payments p ON p.invoice_id = i.id
         WHERE i.id = $1
         GROUP BY i.id, i.student_id, i.title, s.first_name, s.last_name,
                  s.admission_no, i.amount_cents, i.due_date",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let payments = sqlx::query_as::<_, PaymentRow>(
        "SELECT id, invoice_id, amount_cents, method, reference, paid_at, recorded_by
         FROM payments WHERE invoice_id = $1
         ORDER BY paid_at DESC, id DESC",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok((invoice, payments))
}

/// Insert a payment after re-checking the live balance inside this call;
/// overpayments become `BadRequest` (routes turn these into flash messages).
pub async fn record_payment(
    pool: &PgPool,
    invoice_id: i64,
    amount_cents: i64,
    method: &str,
    reference: Option<&str>,
    recorded_by: Option<i64>,
) -> Result<PaymentRow, AppError> {
    let method = method.trim();
    if amount_cents <= 0 {
        return Err(AppError::BadRequest(
            "Payment amount must be greater than zero".to_string(),
        ));
    }
    if !ALLOWED_METHODS.contains(&method) {
        return Err(AppError::BadRequest("Unknown payment method".to_string()));
    }
    // Lock the invoice row and recompute the balance inside one transaction so
    // two concurrent "record payment" clicks can't both pass the check and
    // overpay the invoice into a negative balance.
    let mut tx = pool.begin().await?;
    let locked: Option<(i64,)> = sqlx::query_as("SELECT amount_cents FROM invoices WHERE id = $1 FOR UPDATE")
        .bind(invoice_id)
        .fetch_optional(&mut *tx)
        .await?;
    let amount_owed = locked.ok_or(AppError::NotFound)?.0;
    let paid: i64 =
        sqlx::query_scalar("SELECT (COALESCE(SUM(amount_cents), 0))::BIGINT FROM payments WHERE invoice_id = $1")
            .bind(invoice_id)
            .fetch_one(&mut *tx)
            .await?;
    let fines: i64 =
        sqlx::query_scalar("SELECT (COALESCE(SUM(amount_cents), 0))::BIGINT FROM fee_fines WHERE invoice_id = $1")
            .bind(invoice_id)
            .fetch_one(&mut *tx)
            .await?;
    let balance = amount_owed + fines - paid;
    if amount_cents > balance {
        return Err(AppError::BadRequest(
            "Payment exceeds outstanding balance".to_string(),
        ));
    }
    let reference = reference
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let row = sqlx::query_as::<_, PaymentRow>(
        "INSERT INTO payments (invoice_id, amount_cents, method, reference, recorded_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, invoice_id, amount_cents, method, reference, paid_at, recorded_by",
    )
    .bind(invoice_id)
    .bind(amount_cents)
    .bind(method)
    .bind(reference)
    .bind(recorded_by)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

/// Delete an invoice by id. Payments cascade via the FK, but callers must
/// check [`can_delete_invoice`] first so no payment rows ever exist here.
/// Returns `false` when the id is unknown.
pub async fn delete_invoice(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM invoices WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Total billed / paid / owing for one student (all `::BIGINT`-cast).
pub async fn student_totals(
    pool: &PgPool,
    student_id: i64,
) -> Result<StudentTotals, AppError> {
    let raw = sqlx::query_as::<_, StudentTotalsRaw>(
        "SELECT
            (COALESCE((SELECT SUM(amount_cents) FROM invoices WHERE student_id = $1), 0)
             + COALESCE((SELECT SUM(f.amount_cents) FROM fee_fines f
                         JOIN invoices i ON i.id = f.invoice_id
                         WHERE i.student_id = $1), 0))::BIGINT AS billed,
            (COALESCE((SELECT SUM(p.amount_cents) FROM payments p
                       JOIN invoices i ON i.id = p.invoice_id
                       WHERE i.student_id = $1), 0))::BIGINT AS paid",
    )
    .bind(student_id)
    .fetch_one(pool)
    .await?;
    Ok(StudentTotals {
        billed: raw.billed,
        paid: raw.paid,
        balance: raw.billed - raw.paid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn rejects_empty_title() {
        assert_eq!(
            validate_invoice("", 1000, "2026-09-01"),
            Err("Invoice title is required".to_string())
        );
    }

    #[test]
    fn rejects_blank_title() {
        assert!(validate_invoice("   ", 1000, "2026-09-01").is_err());
    }

    #[test]
    fn rejects_zero_and_negative_amounts() {
        assert_eq!(
            validate_invoice("Tuition", 0, "2026-09-01"),
            Err("Amount must be greater than zero".to_string())
        );
        assert!(validate_invoice("Tuition", -50, "2026-09-01").is_err());
    }

    #[test]
    fn rejects_unparseable_due_date() {
        assert_eq!(
            validate_invoice("Tuition", 1000, "01/09/2026"),
            Err("Due date must be YYYY-MM-DD".to_string())
        );
        assert!(validate_invoice("Tuition", 1000, "").is_err());
        assert!(validate_invoice("Tuition", 1000, "2026-13-40").is_err());
    }

    #[test]
    fn accepts_valid_invoice_and_returns_date() {
        assert_eq!(
            validate_invoice("  Tuition  ", 1500, "2026-09-01"),
            Ok(day(2026, 9, 1))
        );
    }

    #[test]
    fn parses_whole_and_decimal_amounts() {
        assert_eq!(parse_amount_cents("1500"), Ok(150_000));
        assert_eq!(parse_amount_cents("15.00"), Ok(1500));
        assert_eq!(parse_amount_cents("15.5"), Ok(1550));
        assert_eq!(parse_amount_cents("0.99"), Ok(99));
        assert_eq!(parse_amount_cents("1,234.56"), Ok(123_456));
        assert_eq!(parse_amount_cents("  20  "), Ok(2000));
    }

    #[test]
    fn rejects_bad_amount_strings() {
        assert!(parse_amount_cents("").is_err());
        assert!(parse_amount_cents("   ").is_err());
        assert!(parse_amount_cents("abc").is_err());
        assert!(parse_amount_cents("0").is_err());
        assert!(parse_amount_cents("0.00").is_err());
        assert!(parse_amount_cents("-5").is_err());
        assert!(parse_amount_cents("1.234").is_err());
    }

    #[test]
    fn payment_rejects_zero_and_negative() {
        assert!(validate_payment(0, "cash", 1000).is_err());
        assert!(validate_payment(-100, "cash", 1000).is_err());
    }

    #[test]
    fn payment_rejects_unknown_method() {
        assert_eq!(
            validate_payment(500, "cheque", 1000),
            Err("Unknown payment method".to_string())
        );
        assert!(validate_payment(500, "", 1000).is_err());
    }

    #[test]
    fn payment_rejects_overpayment() {
        assert_eq!(
            validate_payment(1001, "cash", 1000),
            Err("Payment exceeds outstanding balance".to_string())
        );
    }

    #[test]
    fn payment_accepts_exact_balance_and_all_methods() {
        for method in ALLOWED_METHODS {
            assert!(validate_payment(1000, method, 1000).is_ok());
        }
        assert!(validate_payment(1, "online", 1000).is_ok());
    }

    #[test]
    fn status_is_paid_when_settled() {
        assert_eq!(invoice_status(0, 1500, day(2026, 9, 1), day(2026, 10, 1)), "paid");
        assert_eq!(
            invoice_status(-100, 1600, day(2026, 9, 1), day(2026, 10, 1)),
            "paid"
        );
    }

    #[test]
    fn status_is_overdue_when_past_due_and_owing() {
        assert_eq!(
            invoice_status(1500, 0, day(2026, 9, 1), day(2026, 10, 1)),
            "overdue"
        );
        assert_eq!(
            invoice_status(500, 1000, day(2026, 9, 1), day(2026, 10, 1)),
            "overdue"
        );
    }

    #[test]
    fn status_is_partial_when_paid_and_not_due() {
        assert_eq!(
            invoice_status(500, 1000, day(2026, 12, 1), day(2026, 10, 1)),
            "partial"
        );
    }

    #[test]
    fn status_is_unpaid_when_nothing_paid_and_not_due() {
        assert_eq!(
            invoice_status(1500, 0, day(2026, 12, 1), day(2026, 10, 1)),
            "unpaid"
        );
    }

    #[test]
    fn allows_delete_when_no_payments_recorded() {
        assert!(can_delete_invoice(0));
    }

    #[test]
    fn rejects_delete_when_any_payment_recorded() {
        assert!(!can_delete_invoice(1));
        assert!(!can_delete_invoice(1500));
    }

    #[test]
    fn treats_negative_paid_as_deletable() {
        assert!(can_delete_invoice(-100));
    }
}
