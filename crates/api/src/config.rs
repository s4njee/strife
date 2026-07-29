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

        Ok(Self {
            database_url,
            storage_root,
            listen_addr,
            tika_url,
        })
    }
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
    use super::validate_http_url;

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
}
