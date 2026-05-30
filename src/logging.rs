use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_logging(debug: bool) -> Result<()> {
    let filter = if debug {
        EnvFilter::new("debug,teatui=trace")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir)
        .wrap_err_with(|| format!("failed to create log directory: {}", log_dir.display()))?;
    let log_path = log_dir.join("app.log");
    let log_file = std::fs::File::create(&log_path)
        .wrap_err_with(|| format!("failed to create log file: {}", log_path.display()))?;

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(log_file)
                .with_ansi(false)
                .with_target(true),
        )
        .init();
    Ok(())
}

fn log_dir() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        return state.join("teatui").join("logs");
    }
    if let Some(home) = dirs::home_dir() {
        return home
            .join(".local")
            .join("state")
            .join("teatui")
            .join("logs");
    }
    PathBuf::from(".").join("teatui").join("logs")
}
