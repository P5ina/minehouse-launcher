use std::sync::Arc;

use serde::Serialize;

/// What the install and launch steps tell the UI.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A new step started, e.g. "Скачиваем Java".
    Stage { name: String },
    /// Bytes (or files) done out of total for the current step.
    Progress { done: u64, total: u64 },
    /// A line for the log panel (also game stdout/stderr).
    Log { line: String },
}

#[derive(Clone)]
pub struct Reporter(Arc<dyn Fn(Event) + Send + Sync>);

impl Reporter {
    pub fn new(f: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
    pub fn silent() -> Self {
        Self::new(|_| {})
    }
    pub fn stage(&self, name: impl Into<String>) {
        (self.0)(Event::Stage { name: name.into() })
    }
    pub fn progress(&self, done: u64, total: u64) {
        (self.0)(Event::Progress { done, total })
    }
    pub fn log(&self, line: impl Into<String>) {
        (self.0)(Event::Log { line: line.into() })
    }
}
