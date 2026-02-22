use std::time::Duration;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::app::Message;

/// Events the TUI can produce.
#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Tick,
    Render,
}

/// Async event handler — polls crossterm events + produces tick/render intervals.
pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    _task: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64, render_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let task = tokio::spawn(async move {
            let mut event_stream = EventStream::new();
            let mut tick_interval = tokio::time::interval(Duration::from_millis(tick_rate_ms));
            let mut render_interval = tokio::time::interval(Duration::from_millis(render_rate_ms));

            loop {
                tokio::select! {
                    maybe_event = event_stream.next() => {
                        if let Some(Ok(CrosstermEvent::Key(key))) = maybe_event {
                            if tx.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                    }
                    _ = tick_interval.tick() => {
                        if tx.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    _ = render_interval.tick() => {
                        if tx.send(Event::Render).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Self { rx, _task: task }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

/// Convert a key event into an app Message.
pub fn key_to_message(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Char('q') => Some(Message::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Message::Quit)
        }
        _ => None,
    }
}
