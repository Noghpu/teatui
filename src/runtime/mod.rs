use color_eyre::eyre::Result;
use crossbeam_channel::{Select, bounded};

use crate::app::App;
use crate::input::{self, InputEvent};
use crate::terminal::Terminal;

pub mod cache;
pub mod http;
pub mod jobs;

pub use cache::Cached;
pub use http::CancelHandle;
pub use jobs::{Job, JobEvent, JobId, JobOutcome, JobOutcomeEvent, JobSubmitter, Jobs};

const WORKER_COUNT: usize = 4;

pub struct Runtime {
    app: App,
    jobs: Jobs,
}

impl Runtime {
    /// Build the runtime, including a fresh worker pool, then hand the
    /// submitter into the supplied factory to construct the App. Boot-time
    /// job submissions live in `App::new`, so by the time `run` is called,
    /// workers may already be processing.
    pub fn new<F>(make_app: F) -> Self
    where
        F: FnOnce(JobSubmitter) -> App,
    {
        let jobs = Jobs::new(WORKER_COUNT);
        let app = make_app(jobs.submitter());
        Self { app, jobs }
    }

    pub fn run(mut self, terminal: &mut Terminal) -> Result<()> {
        let (input_tx, input_rx) = bounded::<InputEvent>(64);
        let _input_handle = input::spawn(input_tx);

        self.draw(terminal)?;
        self.app.clear_dirty();

        loop {
            let mut sel = Select::new();
            let input_idx = sel.recv(&input_rx);
            let jobs_idx = sel.recv(self.jobs.events());
            let op = sel.select();

            match op.index() {
                i if i == input_idx => {
                    let Ok(event) = op.recv(&input_rx) else {
                        tracing::error!(target: "teatui::runtime", "input channel closed");
                        break;
                    };
                    self.app.on_input(event);
                    while let Ok(extra) = input_rx.try_recv() {
                        self.app.on_input(extra);
                    }
                }
                i if i == jobs_idx => {
                    let Ok(event) = op.recv(self.jobs.events()) else {
                        tracing::error!(target: "teatui::runtime", "jobs channel closed");
                        break;
                    };
                    self.app.on_job(event);
                    while let Ok(extra) = self.jobs.events().try_recv() {
                        self.app.on_job(extra);
                    }
                }
                _ => unreachable!(),
            }

            if self.app.should_quit() {
                break;
            }

            if self.app.is_dirty() {
                self.draw(terminal)?;
                self.app.clear_dirty();
            }
        }

        Ok(())
    }

    fn draw(&mut self, terminal: &mut Terminal) -> Result<()> {
        terminal.frame().draw(|frame| self.app.render(frame))?;
        Ok(())
    }
}
