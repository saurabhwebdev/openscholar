//! Fees extensions: categories, discounts, fines, receipts.
//!
//! Runtime SQL only. Money in BIGINT cents.

use chrono::Utc;
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CategoryRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DiscountRow {
    pub id: i64,
    pub name: String,
    pub percent: f64,
    pub student_id: Option<i64>,
    pub category_id: Option<i64>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FineRow {
    pub id: i64,
    pub invoice_id: i64,
    pub amount_cents: i64,
    pub reason: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReceiptRow {
    pub id: i64,
    pub payment_id: i64,
    pub number: String,
    pub issued_at: chrono::DateTime<Utc>,
    pub issued_by: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_category(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Category name is required".to_string());
    }
    Ok(())
}

pub fn validate_discount(name: &str, percent: f64) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Discount name is required".to_string());
    }
    if !(0.0..=100.0).contains(&percent) {
        return Err("Discount percent must be between 0 and 100".to_string());
    }
    Ok(())
}

pub fn validate_discount_str(name: &str, percent: &str) -> Result<(), String> {
    let p: f64 = percent.trim().parse().map_err(|_| "Percent must be a number".to_string())?;
    validate_discount(name, p)
}

pub fn validate_fine(amount_cents: i64, reason: &str) -> Result<(), String> {
    if amount_cents <= 0 {
        return Err("Fine amount must be greater than zero".to_string());
    }
    if reason.trim().is_empty() {
        return Err("Fine reason is required".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Categories CRUD
// ---------------------------------------------------------------------------

pub async fn list_categories(pool: &PgPool) -> Result<Vec<CategoryRow>, AppError> {
    let rows = sqlx::query_as::<_, CategoryRow>(
        "SELECT id, name, description, created_at FROM fee_categories ORDER BY name ASC, id ASC LIMIT 500",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_categories(pool: &PgPool) -> Result<Vec<CategoryRow>, AppError> {
    list_categories(pool).await
}

pub async fn find_category(pool: &PgPool, id: i64) -> Result<Option<CategoryRow>, AppError> {
    let row = sqlx::query_as::<_, CategoryRow>(
        "SELECT id, name, description, created_at FROM fee_categories WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_category(pool: &PgPool, id: i64) -> Result<Option<CategoryRow>, AppError> {
    find_category(pool, id).await
}

pub async fn create_category(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
) -> Result<CategoryRow, AppError> {
    let row = sqlx::query_as::<_, CategoryRow>(
        "INSERT INTO fee_categories (name, description) VALUES ($1, $2)
         RETURNING id, name, description, created_at",
    )
    .bind(name.trim())
    .bind(description.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_category(
    pool: &PgPool,
    id: i64,
    name: &str,
    description: Option<&str>,
) -> Result<bool, AppError> {
    validate_category(name).map_err(AppError::BadRequest)?;
    let result = sqlx::query(
        "UPDATE fee_categories SET name = $2, description = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(name.trim())
    .bind(description.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_category(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM invoices WHERE category_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Err(AppError::BadRequest(
            "Cannot delete a category that is used by invoices".to_string(),
        ));
    }
    let result = sqlx::query("DELETE FROM fee_categories WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Discounts CRUD
// ---------------------------------------------------------------------------

pub async fn list_discounts(pool: &PgPool) -> Result<Vec<DiscountRow>, AppError> {
    let rows = sqlx::query_as::<_, DiscountRow>(
        "SELECT id, name, percent::FLOAT8 AS percent, student_id, category_id, created_at
         FROM fee_discounts ORDER BY id ASC LIMIT 500",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_discounts(pool: &PgPool) -> Result<Vec<DiscountRow>, AppError> {
    list_discounts(pool).await
}

pub async fn find_discount(pool: &PgPool, id: i64) -> Result<Option<DiscountRow>, AppError> {
    let row = sqlx::query_as::<_, DiscountRow>(
        "SELECT id, name, percent::FLOAT8 AS percent, student_id, category_id, created_at
         FROM fee_discounts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_discount(pool: &PgPool, id: i64) -> Result<Option<DiscountRow>, AppError> {
    find_discount(pool, id).await
}

pub async fn create_discount(
    pool: &PgPool,
    name: &str,
    percent: f64,
    student_id: Option<i64>,
    category_id: Option<i64>,
) -> Result<DiscountRow, AppError> {
    validate_discount(name, percent).map_err(AppError::BadRequest)?;
    let row = sqlx::query_as::<_, DiscountRow>(
        "INSERT INTO fee_discounts (name, percent, student_id, category_id)
         VALUES ($1, $2, $3, $4)
         RETURNING id, name, percent::FLOAT8 AS percent, student_id, category_id, created_at",
    )
    .bind(name.trim())
    .bind(percent)
    .bind(student_id)
    .bind(category_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_discount(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM fee_discounts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Fines CRUD
// ---------------------------------------------------------------------------

pub async fn list_fines(pool: &PgPool, invoice_id: i64) -> Result<Vec<FineRow>, AppError> {
    let rows = sqlx::query_as::<_, FineRow>(
        "SELECT id, invoice_id, amount_cents, reason, created_at
         FROM fee_fines WHERE invoice_id = $1 ORDER BY id ASC",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_fines_for_invoice(pool: &PgPool, invoice_id: i64) -> Result<Vec<FineRow>, AppError> {
    list_fines(pool, invoice_id).await
}

pub async fn find_fine(pool: &PgPool, id: i64) -> Result<Option<FineRow>, AppError> {
    let row = sqlx::query_as::<_, FineRow>(
        "SELECT id, invoice_id, amount_cents, reason, created_at FROM fee_fines WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_fine(pool: &PgPool, id: i64) -> Result<Option<FineRow>, AppError> {
    find_fine(pool, id).await
}

pub async fn create_fine(
    pool: &PgPool,
    invoice_id: i64,
    amount_cents: i64,
    reason: &str,
) -> Result<FineRow, AppError> {
    validate_fine(amount_cents, reason).map_err(AppError::BadRequest)?;
    let row = sqlx::query_as::<_, FineRow>(
        "INSERT INTO fee_fines (invoice_id, amount_cents, reason) VALUES ($1, $2, $3)
         RETURNING id, invoice_id, amount_cents, reason, created_at",
    )
    .bind(invoice_id)
    .bind(amount_cents)
    .bind(reason.trim())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_fine(pool: &PgPool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM fee_fines WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

pub async fn receipt_for_payment(pool: &PgPool, payment_id: i64) -> Result<Option<ReceiptRow>, AppError> {
    let row = sqlx::query_as::<_, ReceiptRow>(
        "SELECT id, payment_id, number, issued_at, issued_by FROM receipts WHERE payment_id = $1 ORDER BY id ASC LIMIT 1",
    )
    .bind(payment_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn receipt_number_for_payment(pool: &PgPool, payment_id: i64) -> Result<Option<String>, AppError> {
    Ok(receipt_for_payment(pool, payment_id).await?.map(|r| r.number))
}

pub async fn ensure_receipt<I>(
    pool: &PgPool,
    payment_id: i64,
    issued_by: I,
) -> Result<String, AppError>
where
    I: Into<Option<i64>>,
{
    if let Some(existing) = receipt_for_payment(pool, payment_id).await? {
        return Ok(existing.number);
    }
    let number = format!("R-{payment_id}");
    let issued_by = issued_by.into();
    sqlx::query(
        "INSERT INTO receipts (payment_id, number, issued_by) VALUES ($1, $2, $3)
         ON CONFLICT (number) DO NOTHING",
    )
    .bind(payment_id)
    .bind(&number)
    .bind(issued_by)
    .execute(pool)
    .await?;
    // Re-read in case of race.
    if let Some(existing) = receipt_for_payment(pool, payment_id).await? {
        return Ok(existing.number);
    }
    Ok(number)
}

pub async fn ensure_receipt_row<I>(
    pool: &PgPool,
    payment_id: i64,
    issued_by: I,
) -> Result<ReceiptRow, AppError>
where
    I: Into<Option<i64>>,
{
    ensure_receipt(pool, payment_id, issued_by).await?;
    receipt_for_payment(pool, payment_id).await?.ok_or(AppError::NotFound)
}

// ---------------------------------------------------------------------------
// Apply discount
// ---------------------------------------------------------------------------

pub async fn apply_discount_to_invoice(
    pool: &PgPool,
    invoice_id: i64,
    discount_cents: i64,
) -> Result<(), AppError> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT amount_cents, discount_cents FROM invoices WHERE id = $1",
    )
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?;
    let (amount, prev_discount) = row.ok_or(AppError::NotFound)?;
    let base = amount + prev_discount;
    let new_discount = discount_cents.clamp(0, base);
    let new_amount = (base - new_discount).max(0);
    // Don't let a discount push the invoice below what the family has already
    // paid (which would create an untracked negative balance / hidden credit).
    let paid: i64 = sqlx::query_scalar(
        "SELECT (COALESCE(SUM(amount_cents), 0))::BIGINT FROM payments WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_one(pool)
    .await?;
    let fines: i64 = sqlx::query_scalar(
        "SELECT (COALESCE(SUM(amount_cents), 0))::BIGINT FROM fee_fines WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_one(pool)
    .await?;
    if new_amount + fines < paid {
        return Err(AppError::BadRequest(
            "Discount would drop the invoice below the amount already paid; refund or adjust payments first".to_string(),
        ));
    }
    sqlx::query("UPDATE invoices SET amount_cents = $2, discount_cents = $3 WHERE id = $1")
        .bind(invoice_id)
        .bind(new_amount)
        .bind(new_discount)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn apply_discount_to_invoice_percent(
    pool: &PgPool,
    invoice_id: i64,
    percent: f64,
) -> Result<(), AppError> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT amount_cents, discount_cents FROM invoices WHERE id = $1",
    )
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?;
    let (amount, prev_discount) = row.ok_or(AppError::NotFound)?;
    let base = amount + prev_discount;
    // Integer money math: treat percent as basis points (hundredths of a
    // percent) so the cents multiplication never goes through f64.
    let percent_bp = (percent * 100.0).round().clamp(0.0, 10_000.0) as i64;
    let discount = (((base as i128) * (percent_bp as i128) + 5_000) / 10_000) as i64;
    apply_discount_to_invoice(pool, invoice_id, discount).await
}

pub async fn apply_discount_percent(
    pool: &PgPool,
    invoice_id: i64,
    percent: f64,
) -> Result<(), AppError> {
    apply_discount_to_invoice_percent(pool, invoice_id, percent).await
}

pub async fn apply_discount_by_id(
    pool: &PgPool,
    invoice_id: i64,
    discount_id: i64,
) -> Result<(), AppError> {
    let discount: Option<(f64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT percent::FLOAT8, student_id, category_id FROM fee_discounts WHERE id = $1",
    )
    .bind(discount_id)
    .fetch_optional(pool)
    .await?;
    let (percent, d_student, d_category) = discount.ok_or(AppError::NotFound)?;
    // Enforce the discount's scope: a discount tied to a specific student or fee
    // category must only apply to a matching invoice.
    let inv: Option<(i64, Option<i64>)> =
        sqlx::query_as("SELECT student_id, category_id FROM invoices WHERE id = $1")
            .bind(invoice_id)
            .fetch_optional(pool)
            .await?;
    let (inv_student, inv_category) = inv.ok_or(AppError::NotFound)?;
    if let Some(ds) = d_student {
        if ds != inv_student {
            return Err(AppError::BadRequest(
                "This discount is for a different student".to_string(),
            ));
        }
    }
    if let Some(dc) = d_category {
        if Some(dc) != inv_category {
            return Err(AppError::BadRequest(
                "This discount is for a different fee category".to_string(),
            ));
        }
    }
    apply_discount_to_invoice_percent(pool, invoice_id, percent).await
}

pub async fn apply_discount_to_invoice_with_discount(
    pool: &PgPool,
    invoice_id: i64,
    discount_id: i64,
) -> Result<(), AppError> {
    apply_discount_by_id(pool, invoice_id, discount_id).await
}

pub async fn apply_discount_to_invoice_by_percent(
    pool: &PgPool,
    invoice_id: i64,
    percent: f64,
) -> Result<(), AppError> {
    apply_discount_to_invoice_percent(pool, invoice_id, percent).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_category_rejects_empty() {
        assert!(validate_category("").is_err());
        assert!(validate_category("   ").is_err());
        assert!(validate_category("Books").is_ok());
    }

    #[test]
    fn validate_discount_range() {
        assert!(validate_discount("D", 0.0).is_ok());
        assert!(validate_discount("D", 100.0).is_ok());
        assert!(validate_discount("", 10.0).is_err());
        assert!(validate_discount("D", -1.0).is_err());
        assert!(validate_discount("D", 101.0).is_err());
    }

    #[test]
    fn validate_fine_rules() {
        assert!(validate_fine(100, "late").is_ok());
        assert!(validate_fine(0, "late").is_err());
        assert!(validate_fine(-1, "late").is_err());
        assert!(validate_fine(100, "").is_err());
        assert!(validate_fine(100, "   ").is_err());
    }

    #[test]
    fn receipt_number_format() {
        assert_eq!(format!("R-{}", 5i64), "R-5");
    }
}
