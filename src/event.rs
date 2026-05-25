use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEvent};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::{select, time::interval_at};

use crate::repo::RepoState;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum JobStatus {
    #[default]
    Idle,
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[allow(dead_code)]
pub enum AppEvent {
    Tick,
    Key(KeyEvent),
    Resize(u16, u16),
    Job(JobResult),
    Repo(Box<RepoState>),
}

pub struct EventHandler {
    events: EventStream,
    tick: tokio::time::Interval,
    jobs: UnboundedReceiver<JobResult>,
    repo: UnboundedReceiver<Box<RepoState>>,
}

impl EventHandler {
    pub fn new(
        tick_rate: Duration,
        jobs: UnboundedReceiver<JobResult>,
        repo: UnboundedReceiver<Box<RepoState>>,
    ) -> Self {
        let start = Instant::now() + tick_rate;
        Self {
            events: EventStream::new(),
            tick: interval_at(start.into(), tick_rate),
            jobs,
            repo,
        }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        let events = &mut self.events;

        select! {
            Some(Ok(event)) = events.next() => {
                match event {
                    Event::Key(key) => Ok(AppEvent::Key(key)),
                    Event::Resize(w, h) => Ok(AppEvent::Resize(w, h)),
                    _ => Ok(AppEvent::Tick),
                }
            }
            _ = self.tick.tick() => {
                Ok(AppEvent::Tick)
            }
            Some(job) = self.jobs.recv() => {
                Ok(AppEvent::Job(job))
            }
            Some(repo) = self.repo.recv() => {
                Ok(AppEvent::Repo(repo))
            }
        }
    }
}
