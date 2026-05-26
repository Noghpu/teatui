mod action;
mod app;
mod command;
mod config;
mod context;
mod event;
mod generate;
mod jj;
mod logging;
mod ollama;
mod prompt;
mod repo;
mod tea;
mod tui;
mod ui;

use clap::Parser;
use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::app::App;
use crate::config::Config;
use crate::event::EventHandler;
use crate::logging::init_logging;
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

    let (bg_tx, bg_rx) = mpsc::unbounded_channel();
    let events = EventHandler::new(config.tick_rate, bg_rx);
    let mut app = App::new(config, bg_tx);
    app.refresh();

    let mut tui = Tui::new()?;
    tui.enter()?;
    let result = app.run(&mut tui, events).await;
    tui.exit()?;

    result
}
