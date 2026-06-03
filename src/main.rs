use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, bail, eyre};

use teatui::app::App;
use teatui::config::{self, Config};
use teatui::logging::init_logging;
use teatui::runtime::Runtime;
use teatui::terminal::Terminal;

#[derive(Parser)]
#[command(name = "teatui")]
#[command(about = "Generate Gitea PRs from jj repos with tea and an LLM")]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[arg(short, long)]
    debug: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print an example configuration file to stdout (redirect it to your
    /// config path), or write it there directly with `--write`.
    Config(ConfigArgs),
}

#[derive(clap::Args)]
struct ConfigArgs {
    /// Write the example to the default config path instead of stdout.
    #[arg(short, long)]
    write: bool,
    /// Overwrite an existing file (only meaningful with `--write`).
    #[arg(short, long)]
    force: bool,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    // Subcommands run and exit without starting the TUI, logging, or heartbeat.
    if let Some(Command::Config(args)) = cli.command {
        return run_config(args);
    }

    let config = Config::load(cli.config.as_deref())?;
    init_logging(cli.debug)?;
    tracing::info!(target: "teatui::lifecycle", "starting teatui");

    thread::Builder::new()
        .name("teatui-heartbeat".into())
        .spawn(|| {
            let mut tick: u64 = 0;
            loop {
                thread::sleep(Duration::from_secs(1));
                tick += 1;
                tracing::info!(target: "teatui::heartbeat", tick, "alive");
            }
        })
        .expect("failed to spawn heartbeat thread");

    let mut terminal = Terminal::enter()?;
    let runtime = Runtime::new(|submitter| App::new(config, submitter));
    let result = runtime.run(&mut terminal);
    drop(terminal);

    match &result {
        Ok(()) => tracing::info!(target: "teatui::lifecycle", "exiting teatui"),
        Err(err) => tracing::error!(target: "teatui::lifecycle", error = %err, "exit with error"),
    }
    result
}

/// `teatui config`: emit the annotated example config. Defaults to stdout so it
/// composes with a redirect (`teatui config > path`); `--write` puts it at the
/// default config path, refusing to clobber an existing file without `--force`.
fn run_config(args: ConfigArgs) -> Result<()> {
    let example = config::example_config();
    if !args.write {
        print!("{example}");
        return Ok(());
    }

    let path = config::default_config_path()
        .ok_or_else(|| eyre!("could not determine the config directory on this platform"))?;
    if path.exists() && !args.force {
        bail!(
            "{} already exists; pass --force to overwrite or redirect stdout instead",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| eyre!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, example).map_err(|e| eyre!("could not write {}: {e}", path.display()))?;
    eprintln!("wrote example config to {}", path.display());
    Ok(())
}
