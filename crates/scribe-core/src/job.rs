//! Progress reporting and cancellation shared by all long-running steps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Loading,
    Decoding,
    DetectingSpeech,
    Transcribing,
    Diarizing,
    Finishing,
}

impl Stage {
    /// Share of the total runtime spent before this stage starts, and in it (rough estimate).
    fn span(self) -> (f32, f32) {
        match self {
            Stage::Loading => (0.00, 0.02),
            Stage::Decoding => (0.02, 0.03),
            Stage::DetectingSpeech => (0.05, 0.02),
            Stage::Transcribing => (0.07, 0.78),
            Stage::Diarizing => (0.85, 0.13),
            Stage::Finishing => (0.98, 0.02),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Progress within the stage, `0..=1`.
    pub stage_fraction: f32,
    /// Overall progress, `0..=1`.
    pub overall: f32,
}

#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

pub type ProgressFn = dyn Fn(Progress) + Send + Sync;

#[derive(Clone)]
pub struct JobCtx {
    cancel: Arc<AtomicBool>,
    on_progress: Arc<ProgressFn>,
}

impl JobCtx {
    pub fn new(cancel: Arc<AtomicBool>, on_progress: Arc<ProgressFn>) -> Self {
        Self { cancel, on_progress }
    }

    /// A context that never cancels and ignores progress.
    pub fn detached() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)), Arc::new(|_| {}))
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn report(&self, stage: Stage, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        let (start, len) = stage.span();
        (self.on_progress)(Progress {
            stage,
            stage_fraction: fraction,
            overall: start + len * fraction,
        });
    }
}
