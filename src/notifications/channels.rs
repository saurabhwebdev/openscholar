//! Pluggable notification channels.
//!
//! The in-app inbox (`InAppChannel`) always persists; extra channels
//! (SMS/WhatsApp later) are plain best-effort callbacks fired after persistence,
//! so adding a channel never requires reworking the call sites.

use sqlx::PgPool;

use crate::error::AppError;

/// A delivery channel. Currently `in_app` (persisted) and `log`;
/// future SMS/WhatsApp channels implement this trait and register a callback.
pub trait NotificationChannel {
    fn key(&self) -> &'static str;
}

/// A notification waiting to be delivered.
#[derive(Debug, Clone)]
pub struct PendingNotification {
    pub user_id: i64,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
}

/// Persists notifications into the `notifications` inbox table.
#[derive(Debug, Clone)]
pub struct InAppChannel {
    pub pool: PgPool,
}

impl InAppChannel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn deliver(&self, n: &PendingNotification) -> Result<(), AppError> {
        insert_notification(&self.pool, n).await
    }
}

impl NotificationChannel for InAppChannel {
    fn key(&self) -> &'static str {
        "in_app"
    }
}

/// Logs notifications via `tracing`; never fails.
#[derive(Debug, Clone, Default)]
pub struct LogChannel;

impl LogChannel {
    pub fn new() -> Self {
        Self
    }

    pub fn send(&self, n: &PendingNotification) {
        tracing::info!(
            user_id = n.user_id,
            kind = %n.kind,
            title = %n.title,
            "notification"
        );
    }
}

impl NotificationChannel for LogChannel {
    fn key(&self) -> &'static str {
        "log"
    }
}

async fn insert_notification(pool: &PgPool, n: &PendingNotification) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO notifications (user_id, kind, title, body, link)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(n.user_id)
    .bind(&n.kind)
    .bind(&n.title)
    .bind(&n.body)
    .bind(&n.link)
    .execute(pool)
    .await?;
    Ok(())
}

/// A best-effort extra channel: a plain callback with no persistence
/// and no failure mode. Called synchronously after the inbox write.
pub type ExtraChannel = Box<dyn Fn(&PendingNotification) + Send + Sync>;

/// In-app persistence plus zero or more best-effort extra callbacks.
pub struct ChannelRegistry {
    pub in_app: InAppChannel,
    pub extra: Vec<ExtraChannel>,
}

impl ChannelRegistry {
    pub fn new(in_app: InAppChannel) -> Self {
        Self {
            in_app,
            extra: Vec::new(),
        }
    }

    pub fn with_extra(mut self, channel: ExtraChannel) -> Self {
        self.extra.push(channel);
        self
    }

    /// Fire every extra channel callback. Best-effort: just call them in order.
    pub fn fire_extras(&self, n: &PendingNotification) {
        for channel in &self.extra {
            channel(n);
        }
    }
}

/// Persist `kind`/`title`/`body`/`link` to every recipient's inbox,
/// then fire the registry's extra callbacks for each delivered notification.
pub async fn notify(
    pool: &PgPool,
    registry: &ChannelRegistry,
    user_ids: &[i64],
    kind: &str,
    title: &str,
    body: Option<String>,
    link: Option<String>,
) -> Result<(), AppError> {
    for &user_id in user_ids {
        let pending = PendingNotification {
            user_id,
            kind: kind.to_string(),
            title: title.to_string(),
            body: body.clone(),
            link: link.clone(),
        };
        insert_notification(pool, &pending).await?;
        registry.fire_extras(&pending);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/school")
            .expect("lazy pool parses")
    }

    fn pending(user_id: i64) -> PendingNotification {
        PendingNotification {
            user_id,
            kind: "info".to_string(),
            title: "Hello".to_string(),
            body: Some("Body".to_string()),
            link: Some("/fees".to_string()),
        }
    }

    #[tokio::test]
    async fn channel_keys_are_stable() {
        assert_eq!(InAppChannel::new(lazy_pool()).key(), "in_app");
        assert_eq!(LogChannel::new().key(), "log");
    }

    #[test]
    fn log_channel_send_is_infallible() {
        LogChannel::new().send(&pending(1));
        LogChannel.send(&pending(2));
    }

    #[tokio::test]
    async fn registry_fires_recording_callback() {
        let seen: Arc<Mutex<Vec<(i64, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let probe = Arc::clone(&seen);
        let registry = ChannelRegistry::new(InAppChannel::new(lazy_pool())).with_extra(Box::new(
            move |n: &PendingNotification| {
                probe
                    .lock()
                    .expect("mutex")
                    .push((n.user_id, n.kind.clone(), n.title.clone()));
            },
        ));

        let n = pending(42);
        registry.fire_extras(&n);

        assert_eq!(
            *seen.lock().expect("mutex"),
            vec![(42, "info".to_string(), "Hello".to_string())]
        );
    }

    #[tokio::test]
    async fn notify_with_no_recipients_is_noop() {
        let seen: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        let probe = Arc::clone(&seen);
        let pool = lazy_pool();
        let registry = ChannelRegistry::new(InAppChannel::new(pool.clone())).with_extra(Box::new(
            move |n: &PendingNotification| {
                probe.lock().expect("mutex").push(n.user_id);
            },
        ));

        notify(&pool, &registry, &[], "info", "Hello", None, None)
            .await
            .expect("empty fan-out succeeds without touching the DB");

        assert!(seen.lock().expect("mutex").is_empty());
    }
}
