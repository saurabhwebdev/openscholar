//! Inbox persistence for notifications. Runtime SQL only; invoice-style
//! computed state (read vs unread) comes from `read_at`, never stored twice.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NotificationRow {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationFilter {
    All,
    Unread,
}

impl NotificationFilter {
    pub fn from_param(raw: Option<&str>) -> Self {
        match raw {
            Some("unread") => Self::Unread,
            _ => Self::All,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unread => "unread",
        }
    }
}

pub async fn list_for(
    pool: &PgPool,
    user_id: i64,
    filter: NotificationFilter,
) -> Result<Vec<NotificationRow>, AppError> {
    let rows = match filter {
        NotificationFilter::All => {
            sqlx::query_as::<_, NotificationRow>(
                "SELECT id, user_id, kind, title, body, link, read_at, created_at
                 FROM notifications
                 WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT 200",
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        }
        NotificationFilter::Unread => {
            sqlx::query_as::<_, NotificationRow>(
                "SELECT id, user_id, kind, title, body, link, read_at, created_at
                 FROM notifications
                 WHERE user_id = $1 AND read_at IS NULL
                 ORDER BY created_at DESC, id DESC",
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}

pub async fn unread_count(pool: &PgPool, user_id: i64) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Mark one notification as read. Scoped to the owner: returns `false`
/// (instead of an error) when the id belongs to someone else or is unknown.
pub async fn mark_read(pool: &PgPool, user_id: i64, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE notifications SET read_at = COALESCE(read_at, now())
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark every unread notification of this user as read. Returns rows marked.
pub async fn mark_all_read(pool: &PgPool, user_id: i64) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE notifications SET read_at = COALESCE(read_at, now())
         WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Delete this user's already-read notifications. Scoped to the owner so
/// one user can never clear another user's inbox. Returns rows deleted.
pub async fn clear_read(pool: &PgPool, user_id: i64) -> Result<u64, AppError> {
    let result = sqlx::query(
        "DELETE FROM notifications WHERE user_id = $1 AND read_at IS NOT NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| {
                "postgresql://admin1:dev-local-only@127.0.0.1:5432/school".to_string()
            });
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("test database reachable");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations apply");
        pool
    }

    async fn make_user(pool: &PgPool, email: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO users (email, password_hash, role, full_name)
             VALUES ($1, 'test-hash', 'teacher', 'Notif Test')
             RETURNING id",
        )
        .bind(email)
        .fetch_one(pool)
        .await
        .expect("temp user")
    }

    async fn make_notification(pool: &PgPool, user_id: i64, title: &str, read: bool) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO notifications (user_id, kind, title, body, link, read_at)
             VALUES ($1, 'info', $2, 'body', '/fees', CASE WHEN $3 THEN now() ELSE NULL END)
             RETURNING id",
        )
        .bind(user_id)
        .bind(title)
        .bind(read)
        .fetch_one(pool)
        .await
        .expect("temp notification")
    }

    #[tokio::test]
    async fn inbox_round_trip_with_scoped_reads() {
        let pool = test_pool().await;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let me = make_user(&pool, &format!("notif-a-{nanos}@example.com")).await;
        let other = make_user(&pool, &format!("notif-b-{nanos}@example.com")).await;

        let first = make_notification(&pool, me, "First", false).await;
        make_notification(&pool, me, "Second", false).await;
        make_notification(&pool, me, "Old", true).await;
        let theirs = make_notification(&pool, other, "Theirs", false).await;

        assert_eq!(list_for(&pool, me, NotificationFilter::All).await.unwrap().len(), 3);
        assert_eq!(
            list_for(&pool, me, NotificationFilter::Unread).await.unwrap().len(),
            2
        );
        assert_eq!(unread_count(&pool, me).await.unwrap(), 2);

        // Scoped: cannot mark another user's notification.
        assert!(!mark_read(&pool, me, theirs).await.unwrap());
        assert_eq!(unread_count(&pool, other).await.unwrap(), 1);

        // Unknown id is also `false`, not an error.
        assert!(!mark_read(&pool, me, -1).await.unwrap());

        assert!(mark_read(&pool, me, first).await.unwrap());
        assert_eq!(unread_count(&pool, me).await.unwrap(), 1);

        // Idempotent: re-marking keeps the original read_at, still `true`.
        assert!(mark_read(&pool, me, first).await.unwrap());

        assert_eq!(mark_all_read(&pool, me).await.unwrap(), 1);
        assert_eq!(unread_count(&pool, me).await.unwrap(), 0);
        assert!(list_for(&pool, me, NotificationFilter::Unread).await.unwrap().is_empty());

        // Cleanup temp rows (table is brand-new; users are unique per run).
        sqlx::query("DELETE FROM notifications WHERE user_id = ANY($1)")
            .bind(vec![me, other])
            .execute(&pool)
            .await
            .expect("cleanup notifications");
        sqlx::query("DELETE FROM users WHERE id = ANY($1)")
            .bind(vec![me, other])
            .execute(&pool)
            .await
            .expect("cleanup users");
    }
}
