//! Structured logging — initialization, log codes, and `ecc_log!` macros.
//!
//! # Configuration (env vars)
//!
//! - `ECC_LOG` — level: trace, debug, info, warn, error. Default: `info`
//! - `ECC_LOG_FILE` — path for JSON log file. Set `0` to disable. Default: `~/.config/ecc/ecc.log`
//! - `ECC_LOG_STDERR` — set `0` to disable stderr. Default: on
//!
//! # Usage
//!
//! ```ignore
//! use ecc_core::logging::*;
//!
//! ecc_info!(SRV_STARTUP, "proxy listening on {}", addr);
//! ecc_warn!(FWD_UPSTREAM_5XX, provider = "deepseek", status = 500, "upstream error");
//! ecc_error!(FWD_UPSTREAM_ERROR, provider = "deepseek", "connection failed: {e}");
//! ```
//!
//! Output (stderr):
//! ```text
//! 15:32:01.234  INFO [SRV-001] proxy listening on 127.0.0.1:4010
//! 15:32:05.890  WARN [FWD-004] upstream 5xx │ provider=deepseek status=500
//! 15:32:05.891 ERROR [FWD-003] connection failed │ provider=deepseek error=timeout
//! ```

use std::io;
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

// ── Log code constants ─────────────────────────────────────────────────────

pub const SRV_STARTUP: &str = "[SRV-001]";
pub const SRV_SHUTDOWN: &str = "[SRV-002]";
pub const SRV_CONN_ERROR: &str = "[SRV-003]";

pub const REQ_RECEIVED: &str = "[REQ-001]";
pub const REQ_COMPLETED: &str = "[REQ-002]";
pub const REQ_FAILED: &str = "[REQ-003]";
pub const REQ_BODY_ERROR: &str = "[REQ-004]";

pub const ROUTE_RESOLVED: &str = "[ROUTE-001]";
pub const ROUTE_NOT_FOUND: &str = "[ROUTE-002]";
pub const ROUTE_DATE_FALLBACK: &str = "[ROUTE-003]";

pub const FWD_UPSTREAM_REQUEST: &str = "[FWD-001]";
pub const FWD_UPSTREAM_RESPONSE: &str = "[FWD-002]";
pub const FWD_UPSTREAM_ERROR: &str = "[FWD-003]";
pub const FWD_UPSTREAM_5XX: &str = "[FWD-004]";
pub const FWD_PROVIDER_NOT_FOUND: &str = "[FWD-005]";

pub const FO_TRYING_FALLBACK: &str = "[FO-001]";
pub const FO_ALL_EXHAUSTED: &str = "[FO-002]";

pub const ADM_PROVIDER_CREATED: &str = "[ADM-001]";
pub const ADM_PROVIDER_DELETED: &str = "[ADM-002]";
pub const ADM_ROUTE_CREATED: &str = "[ADM-003]";
pub const ADM_ROUTE_DELETED: &str = "[ADM-004]";
pub const ADM_SAVE_ERROR: &str = "[ADM-005]";

pub const PG_REQUEST: &str = "[PG-001]";
pub const PG_ERROR: &str = "[PG-002]";

pub const USG_RECORDED: &str = "[USG-001]";
pub const USG_PRICING_NOT_FOUND: &str = "[USG-002]";
pub const USG_EXTRACT_FAILED: &str = "[USG-003]";

// ── ECC log macros ─────────────────────────────────────────────────────────

/// Log at INFO level with a structured code.
#[macro_export]
macro_rules! ecc_info {
    ($code:ident, $($arg:tt)*) => {
        $crate::_ecc_log!(info, $code, $($arg)*)
    };
}

/// Log at WARN level with a structured code.
#[macro_export]
macro_rules! ecc_warn {
    ($code:ident, $($arg:tt)*) => {
        $crate::_ecc_log!(warn, $code, $($arg)*)
    };
}

/// Log at ERROR level with a structured code.
#[macro_export]
macro_rules! ecc_error {
    ($code:ident, $($arg:tt)*) => {
        $crate::_ecc_log!(error, $code, $($arg)*)
    };
}

/// Log at DEBUG level with a structured code.
#[macro_export]
macro_rules! ecc_debug {
    ($code:ident, $($arg:tt)*) => {
        $crate::_ecc_log!(debug, $code, $($arg)*)
    };
}

/// Internal: maps a log code constant to the tracing macro, prefixing the message.
#[doc(hidden)]
#[macro_export]
macro_rules! _ecc_log {
    // With structured fields: ecc_info!(CODE, key = val, key2 = val2, "message {}", arg)
    (info, $code:ident, $($key:ident = $val:expr),+ , $($msg:tt)+) => {
        tracing::info!(code = $code, $($key = $val),+, $($msg)+)
    };
    (warn, $code:ident, $($key:ident = $val:expr),+ , $($msg:tt)+) => {
        tracing::warn!(code = $code, $($key = $val),+, $($msg)+)
    };
    (error, $code:ident, $($key:ident = $val:expr),+ , $($msg:tt)+) => {
        tracing::error!(code = $code, $($key = $val),+, $($msg)+)
    };
    (debug, $code:ident, $($key:ident = $val:expr),+ , $($msg:tt)+) => {
        tracing::debug!(code = $code, $($key = $val),+, $($msg)+)
    };
    // Without structured fields: ecc_info!(CODE, "message {}", arg)
    (info, $code:ident, $($msg:tt)+) => {
        tracing::info!(code = $code, $($msg)+)
    };
    (warn, $code:ident, $($msg:tt)+) => {
        tracing::warn!(code = $code, $($msg)+)
    };
    (error, $code:ident, $($msg:tt)+) => {
        tracing::error!(code = $code, $($msg)+)
    };
    (debug, $code:ident, $($msg:tt)+) => {
        tracing::debug!(code = $code, $($msg)+)
    };
}

// ── Initialization ─────────────────────────────────────────────────────────

pub struct LogConfig {
    pub level: String,
    pub log_file: Option<PathBuf>,
    pub stderr: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: "info".into(), log_file: default_log_file(), stderr: true }
    }
}

impl LogConfig {
    pub fn from_env() -> Self {
        let level = std::env::var("ECC_LOG")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "info".into());

        let log_file = match std::env::var("ECC_LOG_FILE") {
            Ok(ref p) if p == "0" || p.is_empty() => None,
            Ok(p) => Some(PathBuf::from(p)),
            Err(_) => default_log_file(),
        };

        let stderr = std::env::var("ECC_LOG_STDERR")
            .map(|v| v != "0").unwrap_or(true);

        Self { level, log_file, stderr }
    }
}

fn default_log_file() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".config").join("ecc");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("ecc.log"))
}

/// Initialize tracing. Call once at startup.
pub fn init(config: LogConfig) {
    let env_filter = EnvFilter::try_new(&config.level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = config.stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_level(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .with_ansi(true)
            .compact()
            .with_writer(io::stderr)
    });

    let file_layer = config.log_file.and_then(|path| {
        let file = std::fs::OpenOptions::new()
            .create(true).append(true)
            .open(&path)
            .map_err(|e| eprintln!("[ecc] cannot open log file {}: {e}", path.display()))
            .ok()?;
        Some(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_level(true)
                .with_current_span(true)
                .with_span_list(false)
                .with_writer(file),
        )
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_config_defaults() {
        let c = LogConfig::default();
        assert_eq!(c.level, "info");
        assert!(c.stderr);
    }

    #[test]
    fn t_config_from_env() {
        std::env::set_var("ECC_LOG", "debug");
        std::env::set_var("ECC_LOG_STDERR", "0");
        let c = LogConfig::from_env();
        assert_eq!(c.level, "debug");
        assert!(!c.stderr);
    }
}
