use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::event::{Event, EventStream, KeyEvent};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::{select, time::interval_at};

use crate::context::ContextResult;
use crate::generate::{GeneratedDraft, RevsetSummary};
use crate::ollama::OllamaError;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationResult {
    Ready(GeneratedDraft),
    Failed(OllamaError),
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
                    Event::Key(key) => Ok(AppEvent::Key(key)),
                    Event::Resize(_, _) => Ok(AppEvent::Resize),
                    _ => Ok(AppEvent::Tick),
                }
            }
            _ = self.tick.tick() => Ok(AppEvent::Tick),
            Some(bg) = self.background.recv() => Ok(AppEvent::Background(bg)),
        }
    }
}
