use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use clap::Parser;
use color_eyre::eyre::Result;

use teatui::app::App;
use teatui::config::Config;
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
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

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
