use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, bail};
use axum::http::Uri;

/// Runtime configuration loaded from environment variables.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub database_url: String,
    pub storage_root: PathBuf,
    pub listen_addr: SocketAddr,
    pub tika_url: String,
    pub upload_session_ttl_hours: u64,
    pub disk_guard_percent: u8,
}

impl Config {
    /// Loads and validates API configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a required variable is missing or malformed.
    pub fn from_env() -> Result<Self> {
        let database_url = required("DATABASE_URL")?;
        let storage_root = PathBuf::from(required("STORAGE_ROOT")?);
        let listen_addr = required("LISTEN_ADDR")?
            .parse()
            .context("LISTEN_ADDR must be a socket address such as 127.0.0.1:3000")?;
        let tika_url = required("TIKA_URL")?;
        validate_http_url("TIKA_URL", &tika_url)?;
        let upload_session_ttl_hours = env::var("UPLOAD_SESSION_TTL_HOURS")
            .unwrap_or_else(|_| "24".to_owned())
            .parse::<u64>()
            .context("UPLOAD_SESSION_TTL_HOURS must be a positive integer")?;
        if upload_session_ttl_hours == 0 {
            bail!("UPLOAD_SESSION_TTL_HOURS must be greater than zero");
        }
        let disk_guard_percent =
            parse_disk_guard_percent(env::var("DISK_GUARD_PERCENT").ok().as_deref())?;

        Ok(Self {
            database_url,
            storage_root,
            listen_addr,
            tika_url,
            upload_session_ttl_hours,
            disk_guard_percent,
        })
    }
}

fn parse_disk_guard_percent(value: Option<&str>) -> Result<u8> {
    let percent = value
        .unwrap_or("90")
        .parse::<u8>()
        .context("DISK_GUARD_PERCENT must be an integer from 1 to 100")?;
    if !(1..=100).contains(&percent) {
        bail!("DISK_GUARD_PERCENT must be between 1 and 100");
    }
    Ok(percent)
}

fn required(name: &str) -> Result<String> {
    let value =
        env::var(name).with_context(|| format!("missing required environment variable {name}"))?;

    if value.trim().is_empty() {
        bail!("environment variable {name} cannot be empty");
    }

    Ok(value)
}

fn validate_http_url(name: &str, value: &str) -> Result<()> {
    let uri: Uri = value
        .parse()
        .with_context(|| format!("{name} must be a valid HTTP URL"))?;

    match uri.scheme_str() {
        Some("http" | "https") if uri.authority().is_some() => Ok(()),
        _ => bail!("{name} must be an absolute HTTP or HTTPS URL"),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_disk_guard_percent, validate_http_url};

    #[test]
    fn accepts_absolute_http_urls() {
        assert!(validate_http_url("TIKA_URL", "http://127.0.0.1:9998").is_ok());
        assert!(validate_http_url("TIKA_URL", "https://tika.local").is_ok());
    }

    #[test]
    fn rejects_relative_or_non_http_urls() {
        assert!(validate_http_url("TIKA_URL", "/version").is_err());
        assert!(validate_http_url("TIKA_URL", "ftp://tika.local").is_err());
    }

    #[test]
    fn disk_guard_defaults_and_validates() {
        assert_eq!(parse_disk_guard_percent(None).expect("default"), 90);
        assert_eq!(parse_disk_guard_percent(Some("87")).expect("custom"), 87);
        assert!(parse_disk_guard_percent(Some("0")).is_err());
        assert!(parse_disk_guard_percent(Some("101")).is_err());
    }
}
