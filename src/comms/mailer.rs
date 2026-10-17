//! Outbound email through a plain SMTP relay (dev default: Mailpit on 1025).
//!
//! Config is runtime-only: [`SmtpConfig::from_env`] reads the process
//! environment, while [`smtp_config_from`] parses a plain map so unit
//! tests never touch env or the network. [`send_one`] is blocking —
//! always call it inside `tokio::task::spawn_blocking`, never on an
//! async worker thread.

use std::collections::HashMap;

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::error::AppError;

/// Runtime SMTP settings. `user`/`pass` are optional: credentials are
/// only sent when both are set (local relays need none).
#[derive(Debug, Clone, PartialEq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub from: String,
}

impl SmtpConfig {
    /// Read `SMTP_*` from the process environment (dev-safe defaults).
    pub fn from_env() -> Self {
        let map: HashMap<String, String> = [
            "SMTP_HOST",
            "SMTP_PORT",
            "SMTP_USER",
            "SMTP_PASS",
            "SMTP_FROM",
        ]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect();
        smtp_config_from(&map)
    }

    /// True when both a username and a password are configured.
    fn has_credentials(&self) -> bool {
        self.user.is_some() && self.pass.is_some()
    }
}

/// Pure parsing with dev-safe defaults: host `localhost`, port `1025`,
/// no credentials unless both user+pass are non-blank, from
/// `school@example.com`. A garbage port falls back to `1025`.
pub fn smtp_config_from(map: &HashMap<String, String>) -> SmtpConfig {
    fn text(map: &HashMap<String, String>, key: &str) -> Option<String> {
        map.get(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    SmtpConfig {
        host: text(map, "SMTP_HOST").unwrap_or_else(|| "localhost".to_string()),
        port: map
            .get("SMTP_PORT")
            .and_then(|s| s.trim().parse::<u16>().ok())
            .unwrap_or(1025),
        user: text(map, "SMTP_USER"),
        pass: text(map, "SMTP_PASS"),
        from: text(map, "SMTP_FROM").unwrap_or_else(|| "school@example.com".to_string()),
    }
}

/// Build a plain-text email. Pure: no network, safe in unit tests.
pub fn build_message(from: &str, to: &str, subject: &str, body: &str) -> Result<Message, String> {
    let from_box: Mailbox = from
        .trim()
        .parse()
        .map_err(|e| format!("invalid sender address: {e}"))?;
    let to_box: Mailbox = to
        .trim()
        .parse()
        .map_err(|e| format!("invalid recipient address: {e}"))?;
    Message::builder()
        .from(from_box)
        .to(to_box)
        .subject(subject)
        .body(body.to_string())
        .map_err(|e| format!("cannot build message: {e}"))
}

/// Local relays speak plaintext; anything else goes through `relay`
/// (STARTTLS). Unit-testable: building never opens a connection.
fn build_transport(config: &SmtpConfig) -> Result<SmtpTransport, String> {
    let host = config.host.trim().to_string();
    // Loopback relays (e.g. Mailpit on localhost) speak plaintext; any real
    // remote relay must use STARTTLS so credentials and parent email content
    // are never sent in the clear.
    let is_local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]");
    let builder = if is_local {
        SmtpTransport::builder_dangerous(host).port(config.port)
    } else {
        SmtpTransport::relay(&host)
            .map_err(|e| format!("cannot configure TLS relay for {host}: {e}"))?
            .port(config.port)
    };
    let builder = if config.has_credentials() {
        builder.credentials(Credentials::new(
            config.user.clone().unwrap_or_default(),
            config.pass.clone().unwrap_or_default(),
        ))
    } else {
        builder
    };
    Ok(builder.build())
}

/// Blocking send of one message. Run inside `spawn_blocking`.
pub fn send_one(config: &SmtpConfig, to: &str, subject: &str, body: &str) -> Result<(), AppError> {
    let msg = build_message(&config.from, to, subject, body).map_err(AppError::Internal)?;
    let transport = build_transport(config).map_err(AppError::Internal)?;
    Transport::send(&transport, &msg)
        .map(|_| ())
        .map_err(|e| AppError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_when_map_empty() {
        let cfg = smtp_config_from(&map(&[]));
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 1025);
        assert_eq!(cfg.user, None);
        assert_eq!(cfg.pass, None);
        assert_eq!(cfg.from, "school@example.com");
        assert!(!cfg.has_credentials());
    }

    #[test]
    fn honors_provided_values() {
        let cfg = smtp_config_from(&map(&[
            ("SMTP_HOST", "mail.example.com"),
            ("SMTP_PORT", "587"),
            ("SMTP_USER", "bot"),
            ("SMTP_PASS", "s3cret"),
            ("SMTP_FROM", "noreply@example.com"),
        ]));
        assert_eq!(cfg.host, "mail.example.com");
        assert_eq!(cfg.port, 587);
        assert_eq!(cfg.user.as_deref(), Some("bot"));
        assert_eq!(cfg.pass.as_deref(), Some("s3cret"));
        assert_eq!(cfg.from, "noreply@example.com");
        assert!(cfg.has_credentials());
    }

    #[test]
    fn trims_values_and_treats_blanks_as_missing() {
        let cfg = smtp_config_from(&map(&[
            ("SMTP_HOST", "  "),
            ("SMTP_USER", "  bot  "),
            ("SMTP_PASS", ""),
            ("SMTP_FROM", "  school@example.com "),
        ]));
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.user.as_deref(), Some("bot"));
        assert_eq!(cfg.pass, None);
        assert_eq!(cfg.from, "school@example.com");
        assert!(!cfg.has_credentials());
    }

    #[test]
    fn bad_port_falls_back_to_default() {
        for raw in ["", "   ", "smtp", "-1", "99999"] {
            let cfg = smtp_config_from(&map(&[("SMTP_PORT", raw)]));
            assert_eq!(cfg.port, 1025, "port {raw:?} should fall back");
        }
    }

    #[test]
    fn credentials_need_both_sides() {
        let only_user = smtp_config_from(&map(&[("SMTP_USER", "bot")]));
        assert!(!only_user.has_credentials());
        let only_pass = smtp_config_from(&map(&[("SMTP_PASS", "s3cret")]));
        assert!(!only_pass.has_credentials());
    }

    #[test]
    fn builds_plain_text_message() {
        let msg = build_message(
            "school@example.com",
            "parent@example.com",
            "Absence alert",
            "Ada was marked absent.",
        )
        .expect("valid addresses build");
        let envelope = msg.envelope();
        assert_eq!(
            envelope.from().expect("from set").to_string(),
            "school@example.com"
        );
        assert_eq!(
            envelope.to(),
            &["parent@example.com"
                .parse::<lettre::Address>()
                .expect("test address parses")]
        );
        let raw = String::from_utf8(msg.formatted()).expect("message formats");
        assert!(raw.contains("Absence alert"));
        assert!(raw.contains("Ada was marked absent."));
    }

    #[test]
    fn rejects_bad_addresses_without_network() {
        assert!(build_message("not-an-email", "p@example.com", "S", "B").is_err());
        assert!(build_message("s@example.com", "nope", "S", "B").is_err());
        assert!(build_message("  ", "p@example.com", "S", "B").is_err());
    }

    #[test]
    fn transport_builds_without_connecting() {
        // `build()` never dials: with and without credentials both succeed.
        let plain = smtp_config_from(&map(&[]));
        assert!(build_transport(&plain).is_ok());
        let authed = smtp_config_from(&map(&[
            ("SMTP_HOST", "mail.example.com"),
            ("SMTP_USER", "bot"),
            ("SMTP_PASS", "s3cret"),
        ]));
        assert!(build_transport(&authed).is_ok());
    }
}
