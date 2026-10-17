//! Library module: books, loans, validation, and runtime-SQL queries.
//!
//! Runtime SQL only. Dates as `NaiveDate`.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BookRow {
    pub id: i64,
    pub isbn: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub copies: i32,
    pub available: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LoanRow {
    pub id: i64,
    pub book_id: i64,
    pub student_id: i64,
    pub borrowed_on: NaiveDate,
    pub due_on: NaiveDate,
    pub returned_on: Option<NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Template-friendly loan view (preformatted dates).
#[derive(Debug, Clone)]
pub struct LoanView {
    pub id: i64,
    pub book_id: i64,
    pub book_title: String,
    pub student_id: i64,
    pub student_name: String,
    pub borrowed_on: String,
    pub due_on: String,
    pub returned_on: String,
    pub overdue: bool,
}

#[derive(Debug, Clone)]
pub struct BookInput {
    pub isbn: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub copies: i32,
    pub available: i32,
}

// ---------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------

pub fn validate_book(title: &str, copies: i32, available: i32) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("Title is required".to_string());
    }
    if copies < 1 {
        return Err("Copies must be at least 1".to_string());
    }
    if available < 0 || available > copies {
        return Err("Available must be between 0 and copies".to_string());
    }
    Ok(())
}

pub fn validate_loan(borrowed_on: NaiveDate, due_on: NaiveDate) -> Result<(), String> {
    if due_on < borrowed_on {
        return Err("Due date must be on or after borrowed date".to_string());
    }
    Ok(())
}

pub fn can_borrow(available: i32) -> bool {
    available > 0
}

pub fn can_return(returned_on: Option<NaiveDate>) -> bool {
    returned_on.is_none()
}

// ---------------------------------------------------------------------------
// Books CRUD
// ---------------------------------------------------------------------------

pub async fn list_books(pool: &PgPool) -> Result<Vec<BookRow>, AppError> {
    let rows = sqlx::query_as::<_, BookRow>(
        "SELECT id, isbn, title, author, category, copies, available, created_at
         FROM books ORDER BY title ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_books(pool: &PgPool) -> Result<Vec<BookRow>, AppError> {
    list_books(pool).await
}

pub async fn find_book(pool: &PgPool, id: i64) -> Result<Option<BookRow>, AppError> {
    let row = sqlx::query_as::<_, BookRow>(
        "SELECT id, isbn, title, author, category, copies, available, created_at
         FROM books WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_book(pool: &PgPool, id: i64) -> Result<Option<BookRow>, AppError> {
    find_book(pool, id).await
}

pub async fn search_books(pool: &PgPool, q: &str) -> Result<Vec<BookRow>, AppError> {
    let pat = format!("%{}%", q.trim());
    let rows = sqlx::query_as::<_, BookRow>(
        "SELECT id, isbn, title, author, category, copies, available, created_at
         FROM books
         WHERE title ILIKE $1 OR author ILIKE $1 OR isbn ILIKE $1 OR category ILIKE $1
         ORDER BY title ASC, id ASC LIMIT 200",
    )
    .bind(pat)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_book(pool: &PgPool, input: &BookInput) -> Result<BookRow, AppError> {
    if let Err(msg) = validate_book(&input.title, input.copies, input.available) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, BookRow>(
        "INSERT INTO books (isbn, title, author, category, copies, available)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, isbn, title, author, category, copies, available, created_at",
    )
    .bind(input.isbn.clone())
    .bind(input.title.trim())
    .bind(input.author.clone())
    .bind(input.category.clone())
    .bind(input.copies)
    .bind(input.available)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_book_fields(
    pool: &PgPool,
    isbn: Option<String>,
    title: &str,
    author: Option<String>,
    category: Option<String>,
    copies: i32,
    available: i32,
) -> Result<BookRow, AppError> {
    let input = BookInput {
        isbn,
        title: title.to_string(),
        author,
        category,
        copies,
        available,
    };
    create_book(pool, &input).await
}

pub async fn update_book(pool: &PgPool, id: i64, input: &BookInput) -> Result<BookRow, AppError> {
    if let Err(msg) = validate_book(&input.title, input.copies, input.available) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, BookRow>(
        "UPDATE books SET isbn = $2, title = $3, author = $4, category = $5, copies = $6, available = $7
         WHERE id = $1
         RETURNING id, isbn, title, author, category, copies, available, created_at",
    )
    .bind(id)
    .bind(input.isbn.clone())
    .bind(input.title.trim())
    .bind(input.author.clone())
    .bind(input.category.clone())
    .bind(input.copies)
    .bind(input.available)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_book(pool: &PgPool, id: i64) -> Result<(), AppError> {
    // blocked if open loans
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM book_loans WHERE book_id = $1 AND returned_on IS NULL",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if open > 0 {
        return Err(AppError::BadRequest(
            "Cannot delete book with open loans".to_string(),
        ));
    }
    let res = sqlx::query("DELETE FROM books WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_book(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_book(pool, id).await
}

// ---------------------------------------------------------------------------
// Borrow / return (transactions)
// ---------------------------------------------------------------------------

pub async fn borrow(
    pool: &PgPool,
    book_id: i64,
    student_id: i64,
    due_on: NaiveDate,
) -> Result<LoanRow, AppError> {
    borrow_on(pool, book_id, student_id, chrono::Local::now().date_naive(), due_on).await
}

pub async fn borrow_on(
    pool: &PgPool,
    book_id: i64,
    student_id: i64,
    borrowed_on: NaiveDate,
    due_on: NaiveDate,
) -> Result<LoanRow, AppError> {
    if let Err(msg) = validate_loan(borrowed_on, due_on) {
        return Err(AppError::BadRequest(msg));
    }
    let mut tx = pool.begin().await?;
    let book = sqlx::query_as::<_, BookRow>(
        "SELECT id, isbn, title, author, category, copies, available, created_at
         FROM books WHERE id = $1 FOR UPDATE",
    )
    .bind(book_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    if !can_borrow(book.available) {
        return Err(AppError::BadRequest("No copies available".to_string()));
    }
    sqlx::query("UPDATE books SET available = available - 1 WHERE id = $1")
        .bind(book_id)
        .execute(&mut *tx)
        .await?;
    let loan = sqlx::query_as::<_, LoanRow>(
        "INSERT INTO book_loans (book_id, student_id, borrowed_on, due_on)
         VALUES ($1, $2, $3, $4)
         RETURNING id, book_id, student_id, borrowed_on, due_on, returned_on, created_at",
    )
    .bind(book_id)
    .bind(student_id)
    .bind(borrowed_on)
    .bind(due_on)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(loan)
}

pub async fn return_book(pool: &PgPool, loan_id: i64) -> Result<LoanRow, AppError> {
    return_book_on(pool, loan_id, chrono::Local::now().date_naive()).await
}

pub async fn return_book_on(
    pool: &PgPool,
    loan_id: i64,
    returned_on: NaiveDate,
) -> Result<LoanRow, AppError> {
    let mut tx = pool.begin().await?;
    let loan = sqlx::query_as::<_, LoanRow>(
        "SELECT id, book_id, student_id, borrowed_on, due_on, returned_on, created_at
         FROM book_loans WHERE id = $1 FOR UPDATE",
    )
    .bind(loan_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    if !can_return(loan.returned_on) {
        return Err(AppError::BadRequest("Loan already returned".to_string()));
    }
    sqlx::query("UPDATE book_loans SET returned_on = $2 WHERE id = $1")
        .bind(loan_id)
        .bind(returned_on)
        .execute(&mut *tx)
        .await?;
    // Clamp to `copies` so returning a loan after copies were manually reduced
    // can't push available above copies (which would violate the CHECK).
    sqlx::query("UPDATE books SET available = LEAST(available + 1, copies) WHERE id = $1")
        .bind(loan.book_id)
        .execute(&mut *tx)
        .await?;
    let updated = sqlx::query_as::<_, LoanRow>(
        "SELECT id, book_id, student_id, borrowed_on, due_on, returned_on, created_at
         FROM book_loans WHERE id = $1",
    )
    .bind(loan_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(updated)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn open_loans_for_student(pool: &PgPool, student_id: i64) -> Result<Vec<LoanView>, AppError> {
    let today = chrono::Local::now().date_naive();
    let rows = sqlx::query_as::<_, (i64, i64, String, i64, String, NaiveDate, NaiveDate, Option<NaiveDate>)>(
        "SELECT bl.id, bl.book_id, b.title, bl.student_id,
                (s.first_name || ' ' || s.last_name) AS student_name,
                bl.borrowed_on, bl.due_on, bl.returned_on
         FROM book_loans bl
         JOIN books b ON b.id = bl.book_id
         JOIN students s ON s.id = bl.student_id
         WHERE bl.student_id = $1 AND bl.returned_on IS NULL
         ORDER BY bl.due_on ASC, bl.id ASC",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LoanView {
            id: r.0,
            book_id: r.1,
            book_title: r.2,
            student_id: r.3,
            student_name: r.4,
            borrowed_on: r.5.to_string(),
            due_on: r.6.to_string(),
            returned_on: r.7.map(|d| d.to_string()).unwrap_or_default(),
            overdue: r.6 < today,
        })
        .collect())
}

pub async fn overdue_loans(pool: &PgPool) -> Result<Vec<LoanView>, AppError> {
    overdue_loans_on(pool, chrono::Local::now().date_naive()).await
}

pub async fn overdue_loans_on(pool: &PgPool, today: NaiveDate) -> Result<Vec<LoanView>, AppError> {
    let rows = sqlx::query_as::<_, (i64, i64, String, i64, String, NaiveDate, NaiveDate, Option<NaiveDate>)>(
        "SELECT bl.id, bl.book_id, b.title, bl.student_id,
                (s.first_name || ' ' || s.last_name) AS student_name,
                bl.borrowed_on, bl.due_on, bl.returned_on
         FROM book_loans bl
         JOIN books b ON b.id = bl.book_id
         JOIN students s ON s.id = bl.student_id
         WHERE bl.returned_on IS NULL AND bl.due_on < $1
         ORDER BY bl.due_on ASC, bl.id ASC",
    )
    .bind(today)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LoanView {
            id: r.0,
            book_id: r.1,
            book_title: r.2,
            student_id: r.3,
            student_name: r.4,
            borrowed_on: r.5.to_string(),
            due_on: r.6.to_string(),
            returned_on: r.7.map(|d| d.to_string()).unwrap_or_default(),
            overdue: true,
        })
        .collect())
}

pub async fn open_loans(pool: &PgPool) -> Result<Vec<LoanView>, AppError> {
    let today = chrono::Local::now().date_naive();
    let rows = sqlx::query_as::<_, (i64, i64, String, i64, String, NaiveDate, NaiveDate, Option<NaiveDate>)>(
        "SELECT bl.id, bl.book_id, b.title, bl.student_id,
                (s.first_name || ' ' || s.last_name) AS student_name,
                bl.borrowed_on, bl.due_on, bl.returned_on
         FROM book_loans bl
         JOIN books b ON b.id = bl.book_id
         JOIN students s ON s.id = bl.student_id
         WHERE bl.returned_on IS NULL
         ORDER BY bl.due_on ASC, bl.id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LoanView {
            id: r.0,
            book_id: r.1,
            book_title: r.2,
            student_id: r.3,
            student_name: r.4,
            borrowed_on: r.5.to_string(),
            due_on: r.6.to_string(),
            returned_on: r.7.map(|d| d.to_string()).unwrap_or_default(),
            overdue: r.6 < today,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn validate_book_requires_title() {
        assert!(validate_book("", 1, 1).is_err());
        assert!(validate_book("   ", 1, 1).is_err());
    }

    #[test]
    fn validate_book_copies_min() {
        assert!(validate_book("T", 0, 0).is_err());
    }

    #[test]
    fn validate_book_available_range() {
        assert!(validate_book("T", 2, 3).is_err());
        assert!(validate_book("T", 2, -1).is_err());
        assert!(validate_book("T", 2, 0).is_ok());
        assert!(validate_book("T", 2, 2).is_ok());
    }

    #[test]
    fn validate_book_ok() {
        assert!(validate_book("Book", 3, 2).is_ok());
    }

    #[test]
    fn validate_loan_due_after_borrowed() {
        let b = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let d = NaiveDate::from_ymd_opt(2024, 1, 10).unwrap();
        assert!(validate_loan(b, d).is_ok());
        assert!(validate_loan(d, b).is_err());
        assert!(validate_loan(b, b).is_ok());
    }

    #[test]
    fn can_borrow_checks_available() {
        assert!(can_borrow(1));
        assert!(!can_borrow(0));
        assert!(!can_borrow(-1));
    }

    #[test]
    fn can_return_checks_not_returned() {
        assert!(can_return(None));
        let d = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        assert!(!can_return(Some(d)));
    }
}
