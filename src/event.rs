use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::{select, time::interval_at};

use crate::context::ContextResult;
use crate::generate::{GeneratedDraft, RevsetSummary, StaleCheckResult};
use crate::llm::LlmError;
use crate::repo::RepoState;

pub enum AppEvent {
    Tick,
    Key(KeyEvent),
    Resize,
    Background(BackgroundEvent),
}

pub enum BackgroundEvent {
    Generation(GenerationResult),
    Context(ContextResult),
    Repo(Box<RepoState>),
    Revsets(Vec<RevsetSummary>),
    StaleCheck(StaleCheckResult),
    Job(JobResult),
    ExecutionStep { index: usize, total: usize },
    ExecutionDone(ExecutionOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationResult {
    Ready(GeneratedDraft),
    Failed(LlmError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
}

impl JobStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed-out",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobResult {
    pub id: u64,
    pub name: String,
    pub command: String,
    pub status: JobStatus,
    pub duration: Option<Duration>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionOutcome {
    pub pr_url: Option<String>,
    pub failed_step: Option<usize>,
    pub message: Option<String>,
}

pub struct EventHandler {
    events: EventStream,
    tick: tokio::time::Interval,
    background: UnboundedReceiver<BackgroundEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, background: UnboundedReceiver<BackgroundEvent>) -> Self {
        let start = Instant::now() + tick_rate;
        Self {
            events: EventStream::new(),
            tick: interval_at(start.into(), tick_rate),
            background,
        }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        let events = &mut self.events;

        select! {
            Some(Ok(event)) = events.next() => {
                match event {
                    Event::Key(key) if key.kind == KeyEventKind::Press => Ok(AppEvent::Key(key)),
                    Event::Resize(_, _) => Ok(AppEvent::Resize),
                    _ => Ok(AppEvent::Tick),
                }
            }
            _ = self.tick.tick() => Ok(AppEvent::Tick),
            Some(bg) = self.background.recv() => Ok(AppEvent::Background(bg)),
        }
    }
}
