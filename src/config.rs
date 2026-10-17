#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub admin_email: String,
    pub admin_password: String,
    pub admin_name: String,
    /// Whether session cookies carry the `Secure` flag (HTTPS-only). Defaults to
    /// true; set `COOKIE_SECURE=false` only for local plain-HTTP development.
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_getter(&|key| std::env::var(key).ok())
    }

    fn from_getter(get: &dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            database_url: get("DATABASE_URL")
                .unwrap_or_else(|| "postgres://localhost/school".to_string()),
            bind_addr: get("BIND_ADDR").unwrap_or_else(|| "0.0.0.0:3000".to_string()),
            admin_email: get("ADMIN_EMAIL")
                .unwrap_or_else(|| "admin@example.com".to_string()),
            admin_password: get("ADMIN_PASSWORD")
                .unwrap_or_else(|| "changeme-admin-123".to_string()),
            admin_name: get("ADMIN_NAME").unwrap_or_else(|| "Administrator".to_string()),
            cookie_secure: get("COOKIE_SECURE")
                .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no"))
                .unwrap_or(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn getter(vars: &HashMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |key| vars.get(key).cloned()
    }

    #[test]
    fn defaults_when_env_missing() {
        let vars = HashMap::new();
        let cfg = Config::from_getter(&getter(&vars));
        assert_eq!(cfg.database_url, "postgres://localhost/school");
        assert_eq!(cfg.bind_addr, "0.0.0.0:3000");
        assert!(!cfg.admin_email.is_empty());
        assert!(!cfg.admin_password.is_empty());
        assert!(cfg.cookie_secure, "cookies must default to Secure");
    }

    #[test]
    fn cookie_secure_can_be_disabled_for_dev() {
        let vars = HashMap::from([("COOKIE_SECURE".to_string(), "false".to_string())]);
        let cfg = Config::from_getter(&getter(&vars));
        assert!(!cfg.cookie_secure);
    }

    #[test]
    fn honors_provided_values() {
        let vars = HashMap::from([
            ("DATABASE_URL".to_string(), "postgres://db/school".to_string()),
            ("BIND_ADDR".to_string(), "127.0.0.1:4000".to_string()),
        ]);
        let cfg = Config::from_getter(&getter(&vars));
        assert_eq!(cfg.database_url, "postgres://db/school");
        assert_eq!(cfg.bind_addr, "127.0.0.1:4000");
    }
}
