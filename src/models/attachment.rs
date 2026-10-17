//! Attachments module: polymorphic document attachments.

use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AttachmentRow {
    pub id: i64,
    pub entity: String,
    pub entity_id: i64,
    pub filename: String,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub storage_key: String,
    pub uploaded_by: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct AttachmentView {
    pub id: i64,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub download_url: String,
    pub created_at: String,
}

impl From<AttachmentRow> for AttachmentView {
    fn from(row: AttachmentRow) -> Self {
        Self {
            id: row.id,
            filename: row.filename,
            content_type: row.content_type.clone().unwrap_or_default(),
            size_bytes: row.size_bytes,
            download_url: format!("/attachments/{}/download", row.id),
            created_at: row.created_at.to_string(),
        }
    }
}

pub fn validate_attachment(entity: &str, filename: &str, size_bytes: i64) -> Result<(), String> {
    if entity.trim().is_empty() {
        return Err("Entity is required".to_string());
    }
    if filename.trim().is_empty() {
        return Err("Filename is required".to_string());
    }
    if filename.chars().count() > 255 {
        return Err("Filename must be at most 255 characters".to_string());
    }
    if size_bytes < 0 {
        return Err("Size must be non-negative".to_string());
    }
    if size_bytes > MAX_ATTACHMENT_BYTES {
        return Err("File exceeds the 10 MB upload limit".to_string());
    }
    Ok(())
}

/// Hard cap on a stored attachment's decoded size.
pub const MAX_ATTACHMENT_BYTES: i64 = 10 * 1024 * 1024;

pub async fn list_for(
    pool: &PgPool,
    entity: &str,
    entity_id: i64,
) -> Result<Vec<AttachmentRow>, AppError> {
    let rows = sqlx::query_as::<_, AttachmentRow>(
        "SELECT id, entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by, created_at
         FROM attachments WHERE entity = $1 AND entity_id = $2 ORDER BY created_at DESC, id DESC",
    )
    .bind(entity)
    .bind(entity_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find(pool: &PgPool, id: i64) -> Result<Option<AttachmentRow>, AppError> {
    let row = sqlx::query_as::<_, AttachmentRow>(
        "SELECT id, entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by, created_at
         FROM attachments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn add(
    pool: &PgPool,
    entity: &str,
    entity_id: i64,
    filename: &str,
    content_type: Option<String>,
    size_bytes: i64,
    storage_key: &str,
    uploaded_by: Option<i64>,
) -> Result<AttachmentRow, AppError> {
    if let Err(msg) = validate_attachment(entity, filename, size_bytes) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, AttachmentRow>(
        "INSERT INTO attachments (entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by, created_at",
    )
    .bind(entity.trim())
    .bind(entity_id)
    .bind(filename.trim())
    .bind(content_type)
    .bind(size_bytes)
    .bind(storage_key)
    .bind(uploaded_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, id: i64) -> Result<Option<AttachmentRow>, AppError> {
    let row = sqlx::query_as::<_, AttachmentRow>(
        "DELETE FROM attachments WHERE id = $1
         RETURNING id, entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by, created_at",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    if let Some(r) = &row {
        // best-effort local file remove
        let path = std::path::Path::new("uploads").join(&r.storage_key);
        let _ = std::fs::remove_file(path);
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_good_attachment() {
        assert!(validate_attachment("student", "file.pdf", 10).is_ok());
    }

    #[test]
    fn rejects_empty_filename() {
        assert!(validate_attachment("student", "", 10).is_err());
    }

    #[test]
    fn rejects_long_filename() {
        let long = "a".repeat(256);
        assert!(validate_attachment("student", &long, 10).is_err());
    }

    #[test]
    fn rejects_negative_size() {
        assert!(validate_attachment("student", "f", -1).is_err());
    }

    #[test]
    fn rejects_empty_entity() {
        assert!(validate_attachment("", "f", 10).is_err());
    }

    #[test]
    fn accepts_zero_size() {
        assert!(validate_attachment("student", "f", 0).is_ok());
    }
}
