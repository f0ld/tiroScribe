//! Transcription queue: jobs run one at a time on a background worker, so several
//! interviews can be lined up (e.g. overnight).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use scribe_core::{Cancelled, Engines, JobCtx, JobSpec, Progress};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize)]
pub struct JobView {
    pub id: u64,
    pub spec: JobSpec,
    pub status: Status,
    pub progress: Option<Progress>,
    pub error: Option<String>,
    /// Seconds since the job started running.
    pub elapsed: Option<f64>,
}

struct Job {
    view: JobView,
    cancel: Arc<AtomicBool>,
    started: Option<Instant>,
}

#[derive(Default)]
pub struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
    next_id: AtomicU64,
}

impl Queue {
    pub fn push(&self, spec: JobSpec) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.jobs.lock().unwrap().push_back(Job {
            view: JobView {
                id,
                spec,
                status: Status::Queued,
                progress: None,
                error: None,
                elapsed: None,
            },
            cancel: Arc::new(AtomicBool::new(false)),
            started: None,
        });
        self.wake.notify_one();
        id
    }

    pub fn views(&self) -> Vec<JobView> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| {
                let mut v = j.view.clone();
                v.elapsed = j.started.map(|s| s.elapsed().as_secs_f64());
                v
            })
            .collect()
    }

    pub fn cancel(&self, id: u64) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|j| j.view.id == id) {
            job.cancel.store(true, Ordering::Relaxed);
            if job.view.status == Status::Queued {
                job.view.status = Status::Cancelled;
            }
        }
    }

    /// Removes a finished job from the list. Running jobs must be cancelled first.
    pub fn remove(&self, id: u64) {
        self.jobs
            .lock()
            .unwrap()
            .retain(|j| j.view.id != id || j.view.status == Status::Running);
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut Job)) -> Option<JobView> {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|j| j.view.id == id)?;
        f(job);
        let mut v = job.view.clone();
        v.elapsed = job.started.map(|s| s.elapsed().as_secs_f64());
        Some(v)
    }

    /// Blocks until a queued job is available and marks it running.
    fn take_next(&self) -> (u64, JobSpec, Arc<AtomicBool>) {
        let mut jobs = self.jobs.lock().unwrap();
        loop {
            if let Some(job) = jobs.iter_mut().find(|j| j.view.status == Status::Queued) {
                job.view.status = Status::Running;
                job.started = Some(Instant::now());
                return (job.view.id, job.view.spec.clone(), job.cancel.clone());
            }
            jobs = self.wake.wait(jobs).unwrap();
        }
    }
}

pub fn spawn_worker(app: AppHandle, queue: Arc<Queue>) {
    std::thread::Builder::new()
        .name("transcription".into())
        .spawn(move || {
            let mut engines = Engines::default();
            loop {
                let (id, spec, cancel) = queue.take_next();
                let _ = app.emit("job-updated", queue.update(id, |_| {}));

                let (q, a) = (queue.clone(), app.clone());
                let last_emit = Mutex::new(Instant::now());
                let ctx = JobCtx::new(
                    cancel,
                    Arc::new(move |p: Progress| {
                        // Throttle UI updates; whisper reports progress very often.
                        let mut last = last_emit.lock().unwrap();
                        if last.elapsed().as_millis() < 250 && p.stage_fraction < 1.0 {
                            return;
                        }
                        *last = Instant::now();
                        let view = q.update(id, |j| j.view.progress = Some(p));
                        let _ = a.emit("job-updated", view);
                    }),
                );

                let result = scribe_core::pipeline::run(&spec, &mut engines, &ctx);
                let view = queue.update(id, |j| match result {
                    Ok(_) => j.view.status = Status::Done,
                    Err(e) if e.is::<Cancelled>() => j.view.status = Status::Cancelled,
                    Err(e) => {
                        log::error!("job {id} failed: {e:#}");
                        j.view.status = Status::Failed;
                        j.view.error = Some(format!("{e:#}"));
                    }
                });
                let _ = app.emit("job-updated", view);
            }
        })
        .expect("spawning worker thread");
}
