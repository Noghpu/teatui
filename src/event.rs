use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEvent};
use futures::StreamExt;
use tokio::{select, time::interval_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobResult {
    pub name: String,
    pub status: JobStatus,
    pub stdout: String,
    pub stderr: String,
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
}

pub struct EventHandler {
    events: EventStream,
    tick: tokio::time::Interval,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let start = Instant::now() + tick_rate;
        Self {
            events: EventStream::new(),
            tick: interval_at(start.into(), tick_rate),
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
        }
    }
}
