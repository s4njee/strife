use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, bail};
use axum::http::Uri;

/// Runtime configuration loaded from environment variables.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub database_url: String,
    pub storage_root: PathBuf,
    pub import_watch_root: PathBuf,
    pub listen_addr: SocketAddr,
    pub tika_url: String,
    pub upload_session_ttl_hours: u64,
    pub disk_guard_percent: u8,
    pub run_migrations: bool,
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
        let import_watch_root =
            parse_import_watch_root(env::var("IMPORT_WATCH_ROOT").ok().as_deref())?;
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
        let run_migrations = parse_bool(
            "RUN_MIGRATIONS",
            env::var("RUN_MIGRATIONS").ok().as_deref(),
            true,
        )?;

        Ok(Self {
            database_url,
            storage_root,
            import_watch_root,
            listen_addr,
            tika_url,
            upload_session_ttl_hours,
            disk_guard_percent,
            run_migrations,
        })
    }
}

fn parse_import_watch_root(value: Option<&str>) -> Result<PathBuf> {
    let value = value.unwrap_or("/mnt/ext/watch");
    if value.trim().is_empty() {
        bail!("IMPORT_WATCH_ROOT cannot be empty");
    }
    Ok(PathBuf::from(value))
}

fn parse_bool(name: &str, value: Option<&str>, default: bool) -> Result<bool> {
    match value {
        None => Ok(default),
        Some("true" | "1" | "yes" | "on") => Ok(true),
        Some("false" | "0" | "no" | "off") => Ok(false),
        Some(_) => bail!("{name} must be true or false"),
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
    use std::path::PathBuf;

    use super::{parse_bool, parse_disk_guard_percent, parse_import_watch_root, validate_http_url};

    #[test]
    fn import_watch_root_defaults_overrides_and_rejects_empty_values() {
        assert_eq!(
            parse_import_watch_root(None).expect("default"),
            PathBuf::from("/mnt/ext/watch")
        );
        assert_eq!(
            parse_import_watch_root(Some("./.data/import")).expect("override"),
            PathBuf::from("./.data/import")
        );
        assert!(parse_import_watch_root(Some("  ")).is_err());
    }

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

    #[test]
    fn migration_toggle_defaults_on_and_parses_explicit_values() {
        assert!(parse_bool("RUN_MIGRATIONS", None, true).expect("default"));
        assert!(!parse_bool("RUN_MIGRATIONS", Some("false"), true).expect("disabled"));
        assert!(parse_bool("RUN_MIGRATIONS", Some("1"), false).expect("enabled"));
        assert!(parse_bool("RUN_MIGRATIONS", Some("sometimes"), true).is_err());
    }
}
