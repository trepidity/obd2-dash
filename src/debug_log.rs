use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

const MAX_LINES: usize = 2000;

/// Thread-safe ring buffer that stores formatted log lines.
#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LINES))),
        }
    }

    pub fn push(&self, line: String) {
        let mut buf = self.inner.lock().unwrap();
        if buf.len() >= MAX_LINES {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.inner.lock().unwrap().iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

/// Extracts the message field (and any additional fields) from a tracing event.
struct MessageVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: Vec::new(),
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields
                .push((field.name().to_string(), format!("{:?}", value)));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

/// A tracing Layer that formats events and pushes them into a LogBuffer.
pub struct BufferLayer {
    buffer: LogBuffer,
}

impl BufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = meta.level();
        let target = meta.target();

        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        let now = chrono::Local::now();
        let timestamp = now.format("%H:%M:%S%.3f");

        let mut line = format!("{} {:>5} {}: {}", timestamp, level, target, visitor.message);

        for (key, val) in &visitor.fields {
            line.push_str(&format!(" {}={}", key, val));
        }

        self.buffer.push(line);
    }
}
