//! Logging infrastructure for Imp.
//!
//! Logs go to `~/.imp/logs/imp.log` (rotated daily). Nothing is printed to
//! stderr/stdout — the terminal stays clean for the user.
//!
//! Log level is controlled by `IMP_LOG` env var (default: `info`).
//! Examples: `IMP_LOG=debug`, `IMP_LOG=warn`, `IMP_LOG=imp::tools::mcp=debug`.

use crate::config::imp_home;
use tracing_appender::rolling;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Initialise the global logger. Safe to call multiple times (subsequent
/// calls are no-ops because `tracing_subscriber::registry().init()` only
/// takes effect once).
pub fn init() {
    let home = match imp_home() {
        Ok(h) => h,
        Err(_) => return, // Can't log if we don't know where home is
    };

    let log_dir = home.join("logs");
    if let Err(_) = std::fs::create_dir_all(&log_dir) {
        return;
    }

    let file_appender = rolling::daily(&log_dir, "imp.log");

    let filter = EnvFilter::try_from_env("IMP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = fmt::layer()
        .with_writer(file_appender)
        .with_target(true)
        .with_ansi(false);

    // This silently no-ops if a subscriber is already set (e.g. in tests)
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();
}
