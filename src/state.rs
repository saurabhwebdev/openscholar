use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::ratelimit::IpLimiter;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    /// In-memory per-IP login throttle (complements the DB per-account lockout).
    pub login_limiter: Arc<IpLimiter>,
}
