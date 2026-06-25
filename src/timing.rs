use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct DebugTimer {
    label: String,
    started: Option<Instant>,
}

impl DebugTimer {
    pub(crate) fn start(label: impl Into<String>) -> Self {
        let label = label.into();
        log::debug!("{label} started");
        Self {
            label,
            started: Some(Instant::now()),
        }
    }

    pub(crate) fn finish(mut self) -> Duration {
        let elapsed = self
            .started
            .take()
            .map(|started| started.elapsed())
            .unwrap_or_default();
        log::debug!("{} completed in {:.3?}", self.label, elapsed);
        elapsed
    }
}

impl Drop for DebugTimer {
    fn drop(&mut self) {
        if let Some(started) = self.started.take() {
            log::debug!("{} completed in {:.3?}", self.label, started.elapsed());
        }
    }
}
