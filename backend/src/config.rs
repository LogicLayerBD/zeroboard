use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_JWT_EXPIRY_MINUTES: i64 = 15;
const DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS: i64 = 30;
const DEFAULT_DATABASE_URL: &str = "sqlite://data/zeroboard.db";
const DEFAULT_ATTACHMENTS_DIR: &str = "data/attachments";
const DEFAULT_MAX_ATTACHMENT_SIZE_MB: u64 = 25;
const DEFAULT_APP_NAME: &str = "ZeroBoard";
const DEFAULT_FIRST_USER_IS_ADMIN: bool = true;

/// 32 bytes is the minimum key length recommended for HMAC-SHA256 JWT signing.
const MIN_JWT_SECRET_LEN: usize = 32;
/// Placeholder shipped in `.env.example`; refusing it prevents running with a public secret.
const EXAMPLE_JWT_SECRET: &str = "change-this-to-a-long-random-string";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for environment variable {0}")]
    Invalid(&'static str),
    #[error("JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} characters and not the example value")]
    WeakJwtSecret,
}

#[derive(Clone)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub jwt_secret: String,
    pub jwt_expiry_minutes: i64,
    pub refresh_token_expiry_days: i64,
    pub database_url: String,
    pub attachments_dir: PathBuf,
    pub max_attachment_size_mb: u64,
    pub app_name: String,
    pub first_user_is_admin: bool,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("jwt_secret", &"<redacted>")
            .field("jwt_expiry_minutes", &self.jwt_expiry_minutes)
            .field("refresh_token_expiry_days", &self.refresh_token_expiry_days)
            .field("database_url", &self.database_url)
            .field("attachments_dir", &self.attachments_dir)
            .field("max_attachment_size_mb", &self.max_attachment_size_mb)
            .field("app_name", &self.app_name)
            .field("first_user_is_admin", &self.first_user_is_admin)
            .finish()
    }
}

/// Database location only, for CLI commands (backup/restore) that must not
/// require the server's secrets.
pub fn database_url_from_env() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let jwt_secret = lookup("JWT_SECRET").ok_or(ConfigError::Missing("JWT_SECRET"))?;
        if jwt_secret.len() < MIN_JWT_SECRET_LEN || jwt_secret == EXAMPLE_JWT_SECRET {
            return Err(ConfigError::WeakJwtSecret);
        }

        let jwt_expiry_minutes =
            parse_or(&lookup, "JWT_EXPIRY_MINUTES", DEFAULT_JWT_EXPIRY_MINUTES)?;
        let refresh_token_expiry_days = parse_or(
            &lookup,
            "REFRESH_TOKEN_EXPIRY_DAYS",
            DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS,
        )?;
        if jwt_expiry_minutes <= 0 {
            return Err(ConfigError::Invalid("JWT_EXPIRY_MINUTES"));
        }
        if refresh_token_expiry_days <= 0 {
            return Err(ConfigError::Invalid("REFRESH_TOKEN_EXPIRY_DAYS"));
        }

        let max_attachment_size_mb = parse_or(
            &lookup,
            "MAX_ATTACHMENT_SIZE_MB",
            DEFAULT_MAX_ATTACHMENT_SIZE_MB,
        )?;
        if max_attachment_size_mb == 0 {
            return Err(ConfigError::Invalid("MAX_ATTACHMENT_SIZE_MB"));
        }

        Ok(Self {
            host: parse_or(&lookup, "HOST", IpAddr::from_str(DEFAULT_HOST).expect("valid default host"))?,
            port: parse_or(&lookup, "PORT", DEFAULT_PORT)?,
            jwt_secret,
            jwt_expiry_minutes,
            refresh_token_expiry_days,
            database_url: lookup("DATABASE_URL").unwrap_or_else(|| DEFAULT_DATABASE_URL.to_string()),
            attachments_dir: lookup("ATTACHMENTS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_ATTACHMENTS_DIR)),
            max_attachment_size_mb,
            app_name: lookup("APP_NAME").unwrap_or_else(|| DEFAULT_APP_NAME.to_string()),
            first_user_is_admin: parse_or(
                &lookup,
                "FIRST_USER_IS_ADMIN",
                DEFAULT_FIRST_USER_IS_ADMIN,
            )?,
        })
    }
}

fn parse_or<T: FromStr>(
    lookup: &impl Fn(&str) -> Option<String>,
    key: &'static str,
    default: T,
) -> Result<T, ConfigError> {
    match lookup(key) {
        Some(raw) => raw.trim().parse().map_err(|_| ConfigError::Invalid(key)),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const VALID_SECRET: &str = "a-test-secret-that-is-at-least-32-chars";

    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn applies_defaults_when_only_secret_is_set() {
        let config = Config::from_lookup(lookup_from(&[("JWT_SECRET", VALID_SECRET)])).unwrap();
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.host.to_string(), DEFAULT_HOST);
        assert_eq!(config.jwt_expiry_minutes, DEFAULT_JWT_EXPIRY_MINUTES);
        assert_eq!(config.refresh_token_expiry_days, DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS);
        assert_eq!(config.database_url, DEFAULT_DATABASE_URL);
        assert_eq!(config.attachments_dir, PathBuf::from(DEFAULT_ATTACHMENTS_DIR));
        assert_eq!(config.max_attachment_size_mb, DEFAULT_MAX_ATTACHMENT_SIZE_MB);
        assert_eq!(config.app_name, DEFAULT_APP_NAME);
        assert!(config.first_user_is_admin);
    }

    #[test]
    fn reads_overrides() {
        let config = Config::from_lookup(lookup_from(&[
            ("JWT_SECRET", VALID_SECRET),
            ("PORT", "8080"),
            ("HOST", "127.0.0.1"),
            ("FIRST_USER_IS_ADMIN", "false"),
            ("MAX_ATTACHMENT_SIZE_MB", "10"),
        ]))
        .unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host.to_string(), "127.0.0.1");
        assert!(!config.first_user_is_admin);
        assert_eq!(config.max_attachment_size_mb, 10);
    }

    #[test]
    fn requires_jwt_secret() {
        let err = Config::from_lookup(lookup_from(&[])).unwrap_err();
        assert_eq!(err, ConfigError::Missing("JWT_SECRET"));
    }

    #[test]
    fn rejects_short_or_example_secret() {
        let short = Config::from_lookup(lookup_from(&[("JWT_SECRET", "short")])).unwrap_err();
        assert_eq!(short, ConfigError::WeakJwtSecret);

        let example =
            Config::from_lookup(lookup_from(&[("JWT_SECRET", EXAMPLE_JWT_SECRET)])).unwrap_err();
        assert_eq!(example, ConfigError::WeakJwtSecret);
    }

    #[test]
    fn rejects_unparseable_and_non_positive_values() {
        let bad_port = Config::from_lookup(lookup_from(&[
            ("JWT_SECRET", VALID_SECRET),
            ("PORT", "not-a-port"),
        ]))
        .unwrap_err();
        assert_eq!(bad_port, ConfigError::Invalid("PORT"));

        let zero_expiry = Config::from_lookup(lookup_from(&[
            ("JWT_SECRET", VALID_SECRET),
            ("JWT_EXPIRY_MINUTES", "0"),
        ]))
        .unwrap_err();
        assert_eq!(zero_expiry, ConfigError::Invalid("JWT_EXPIRY_MINUTES"));
    }

    #[test]
    fn debug_output_redacts_secret() {
        let config = Config::from_lookup(lookup_from(&[("JWT_SECRET", VALID_SECRET)])).unwrap();
        let rendered = format!("{config:?}");
        assert!(!rendered.contains(VALID_SECRET));
        assert!(rendered.contains("<redacted>"));
    }
}
