use chrono::NaiveDate;
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LessonRow {
    pub id: i64,
    pub class_id: i64,
    pub subject_id: Option<i64>,
    pub teacher_id: Option<i64>,
    pub date: NaiveDate,
    pub title: String,
    pub body: Option<String>,
    pub homework: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MaterialRow {
    pub id: i64,
    pub lesson_id: i64,
    pub filename: String,
    pub storage_key: String,
    pub size_bytes: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct LessonInput {
    pub class_id: i64,
    pub subject_id: Option<i64>,
    pub teacher_id: Option<i64>,
    pub date: NaiveDate,
    pub title: String,
    pub body: Option<String>,
    pub homework: Option<String>,
}

pub fn validate_lesson(title: &str, date: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("Title is required".to_string());
    }
    NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "Date must be YYYY-MM-DD".to_string())
}

// aliases
pub fn validate_lesson_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        Err("Title is required".to_string())
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Lessons CRUD
// ---------------------------------------------------------------------------

pub async fn list_lessons(pool: &PgPool) -> Result<Vec<LessonRow>, AppError> {
    let rows = sqlx::query_as::<_, LessonRow>(
        "SELECT id, class_id, subject_id, teacher_id, date, title, body, homework, created_at
         FROM lessons ORDER BY date DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_for_class(pool: &PgPool, class_id: i64) -> Result<Vec<LessonRow>, AppError> {
    let rows = sqlx::query_as::<_, LessonRow>(
        "SELECT id, class_id, subject_id, teacher_id, date, title, body, homework, created_at
         FROM lessons WHERE class_id = $1 ORDER BY date DESC, id DESC",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn lessons_for_class(pool: &PgPool, class_id: i64) -> Result<Vec<LessonRow>, AppError> {
    list_for_class(pool, class_id).await
}

pub async fn list_for_student(pool: &PgPool, student_id: i64) -> Result<Vec<LessonRow>, AppError> {
    let rows = sqlx::query_as::<_, LessonRow>(
        "SELECT l.id, l.class_id, l.subject_id, l.teacher_id, l.date, l.title, l.body, l.homework, l.created_at
         FROM lessons l
         JOIN enrollments e ON e.class_id = l.class_id
         WHERE e.student_id = $1
         ORDER BY l.date DESC, l.id DESC",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn lessons_for_student(pool: &PgPool, student_id: i64) -> Result<Vec<LessonRow>, AppError> {
    list_for_student(pool, student_id).await
}

pub async fn find_lesson(pool: &PgPool, id: i64) -> Result<Option<LessonRow>, AppError> {
    let row = sqlx::query_as::<_, LessonRow>(
        "SELECT id, class_id, subject_id, teacher_id, date, title, body, homework, created_at
         FROM lessons WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_lesson(pool: &PgPool, id: i64) -> Result<Option<LessonRow>, AppError> {
    find_lesson(pool, id).await
}

pub async fn create_lesson(pool: &PgPool, input: &LessonInput) -> Result<LessonRow, AppError> {
    let row = sqlx::query_as::<_, LessonRow>(
        "INSERT INTO lessons (class_id, subject_id, teacher_id, date, title, body, homework)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, class_id, subject_id, teacher_id, date, title, body, homework, created_at",
    )
    .bind(input.class_id)
    .bind(input.subject_id)
    .bind(input.teacher_id)
    .bind(input.date)
    .bind(input.title.trim())
    .bind(input.body.clone())
    .bind(input.homework.clone())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_lesson_fields(
    pool: &PgPool,
    class_id: i64,
    subject_id: Option<i64>,
    teacher_id: Option<i64>,
    date: NaiveDate,
    title: &str,
    body: Option<String>,
    homework: Option<String>,
) -> Result<LessonRow, AppError> {
    let input = LessonInput {
        class_id,
        subject_id,
        teacher_id,
        date,
        title: title.to_string(),
        body,
        homework,
    };
    create_lesson(pool, &input).await
}

// aliases
pub async fn add_lesson(pool: &PgPool, input: &LessonInput) -> Result<LessonRow, AppError> {
    create_lesson(pool, input).await
}
pub async fn insert_lesson(pool: &PgPool, input: &LessonInput) -> Result<LessonRow, AppError> {
    create_lesson(pool, input).await
}

pub async fn update_lesson(pool: &PgPool, id: i64, input: &LessonInput) -> Result<LessonRow, AppError> {
    let row = sqlx::query_as::<_, LessonRow>(
        "UPDATE lessons SET class_id=$2, subject_id=$3, teacher_id=$4, date=$5, title=$6, body=$7, homework=$8
         WHERE id=$1
         RETURNING id, class_id, subject_id, teacher_id, date, title, body, homework, created_at",
    )
    .bind(id)
    .bind(input.class_id)
    .bind(input.subject_id)
    .bind(input.teacher_id)
    .bind(input.date)
    .bind(input.title.trim())
    .bind(input.body.clone())
    .bind(input.homework.clone())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_lesson(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM lessons WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_lesson(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_lesson(pool, id).await
}

// ---------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------

pub async fn list_materials(pool: &PgPool, lesson_id: i64) -> Result<Vec<MaterialRow>, AppError> {
    let rows = sqlx::query_as::<_, MaterialRow>(
        "SELECT id, lesson_id, filename, storage_key, size_bytes, created_at
         FROM lesson_materials WHERE lesson_id = $1 ORDER BY id",
    )
    .bind(lesson_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn materials_for_lesson(pool: &PgPool, lesson_id: i64) -> Result<Vec<MaterialRow>, AppError> {
    list_materials(pool, lesson_id).await
}

pub async fn add_material(
    pool: &PgPool,
    lesson_id: i64,
    filename: &str,
    storage_key: &str,
    size_bytes: i64,
) -> Result<MaterialRow, AppError> {
    let row = sqlx::query_as::<_, MaterialRow>(
        "INSERT INTO lesson_materials (lesson_id, filename, storage_key, size_bytes)
         VALUES ($1, $2, $3, $4)
         RETURNING id, lesson_id, filename, storage_key, size_bytes, created_at",
    )
    .bind(lesson_id)
    .bind(filename.trim())
    .bind(storage_key.trim())
    .bind(size_bytes)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn add_lesson_material(
    pool: &PgPool,
    lesson_id: i64,
    filename: &str,
    storage_key: &str,
    size_bytes: i64,
) -> Result<MaterialRow, AppError> {
    add_material(pool, lesson_id, filename, storage_key, size_bytes).await
}

pub async fn create_material(
    pool: &PgPool,
    lesson_id: i64,
    filename: &str,
    storage_key: &str,
    size_bytes: i64,
) -> Result<MaterialRow, AppError> {
    add_material(pool, lesson_id, filename, storage_key, size_bytes).await
}

pub async fn delete_material(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM lesson_materials WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_material(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_material(pool, id).await
}

pub async fn find_material(pool: &PgPool, id: i64) -> Result<Option<MaterialRow>, AppError> {
    let row = sqlx::query_as::<_, MaterialRow>(
        "SELECT id, lesson_id, filename, storage_key, size_bytes, created_at
         FROM lesson_materials WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}