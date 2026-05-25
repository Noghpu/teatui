mod action;
mod app;
mod command;
mod config;
mod event;
mod generate;
mod logging;
mod repo;
mod tui;
mod ui;

use clap::Parser;
use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::app::App;
use crate::command::CommandRunner;
use crate::config::Config;
use crate::event::EventHandler;
use crate::logging::init_logging;
use crate::repo::RepoDiscovery;
use crate::tui::Tui;

#[derive(Parser)]
#[command(name = "teatui")]
#[command(about = "Generate Gitea PRs from jj repos with tea and Ollama")]
struct Cli {
    /// Config file path
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    let config = Config::load(cli.config.as_deref())?;
    init_logging(&config, cli.debug)?;

    tracing::info!("Starting application");

    let mut tui = Tui::new()?;
    let (job_tx, job_rx) = mpsc::unbounded_channel();
    let (repo_tx, repo_rx) = mpsc::unbounded_channel();
    let runner = CommandRunner::new(&config, job_tx);
    let discovery = RepoDiscovery::new(config.clone(), repo_tx);
    let events = EventHandler::new(config.tick_rate, job_rx, repo_rx);
    let mut app = App::new(config, runner, discovery);
    app.refresh();

    tui.enter()?;
    let result = app.run(&mut tui, events).await;
    tui.exit()?;

    result
}
