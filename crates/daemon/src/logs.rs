//! A tracing layer that keeps recent log lines for `GET /api/logs` and
//! streams them to WebSocket clients.

use cseek_core::LogEntry;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

const CAPACITY: usize = 1000;

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<LogEntry>>>,
    tx: broadcast::Sender<LogEntry>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(VecDeque::with_capacity(CAPACITY))), tx: broadcast::channel(1024).0 }
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.inner.lock().iter().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }

    fn push(&self, e: LogEntry) {
        {
            let mut q = self.inner.lock();
            if q.len() == CAPACITY {
                q.pop_front();
            }
            q.push_back(e.clone());
        }
        let _ = self.tx.send(e);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }
}

pub struct BufferLayer {
    pub buffer: LogBuffer,
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let m = event.metadata();
        if !(*m.level() <= Level::INFO && (m.target().starts_with("cseek") || m.target().starts_with("slsk"))) {
            return;
        }
        let mut v = MessageVisitor::default();
        event.record(&mut v);
        self.buffer.push(LogEntry {
            timestamp: chrono::Utc::now(),
            level: m.level().to_string(),
            target: m.target().to_owned(),
            message: v.message + &v.fields,
        });
    }
}
