use std::thread::{self, JoinHandle};

use crossbeam_channel::Sender;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};

#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(KeyEvent),
    Resize { width: u16, height: u16 },
}

pub fn spawn(tx: Sender<InputEvent>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("teatui-input".into())
        .spawn(move || run(tx))
        .expect("failed to spawn input thread")
}

fn run(tx: Sender<InputEvent>) {
    loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                if tx.send(InputEvent::Key(key)).is_err() {
                    return;
                }
            }
            Ok(Event::Resize(width, height)) => {
                if tx.send(InputEvent::Resize { width, height }).is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(target: "teatui::input", error = %e, "read failed");
                return;
            }
        }
    }
}
