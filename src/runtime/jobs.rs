use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

/// A unit of background work. Runs on a worker thread; the boxed `Any`
/// payload it returns is delivered to the runtime as a `JobEvent` and
/// downcast by the consumer.
pub trait Job: Send + 'static {
    /// Short, stable name for logs.
    fn name(&self) -> &'static str;
    fn run(self: Box<Self>) -> JobOutcome;
}

pub enum JobOutcome {
    Done(Box<dyn Any + Send>),
    Failed(String),
}

#[derive(Debug)]
pub struct JobEvent {
    pub id: JobId,
    pub name: &'static str,
    pub outcome: JobOutcomeEvent,
}

pub enum JobOutcomeEvent {
    Done(Box<dyn Any + Send>),
    Failed(String),
}

impl std::fmt::Debug for JobOutcomeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobOutcomeEvent::Done(_) => f.write_str("Done(..)"),
            JobOutcomeEvent::Failed(msg) => write!(f, "Failed({msg:?})"),
        }
    }
}

pub struct Jobs {
    submit_tx: Sender<Submission>,
    events_rx: Receiver<JobEvent>,
    next_id: Arc<AtomicU64>,
    _workers: Vec<JoinHandle<()>>,
}

/// Cheaply-cloneable handle for submitting jobs from anywhere.
#[derive(Clone)]
pub struct JobSubmitter {
    submit_tx: Sender<Submission>,
    next_id: Arc<AtomicU64>,
}

struct Submission {
    id: JobId,
    job: Box<dyn JobErased>,
}

trait JobErased: Send {
    fn name(&self) -> &'static str;
    fn run_boxed(self: Box<Self>) -> JobOutcome;
}

impl<J: Job> JobErased for J {
    fn name(&self) -> &'static str {
        Job::name(self)
    }
    fn run_boxed(self: Box<Self>) -> JobOutcome {
        Job::run(self)
    }
}

impl Jobs {
    pub fn new(workers: usize) -> Self {
        let (submit_tx, submit_rx) = unbounded::<Submission>();
        let (events_tx, events_rx) = unbounded::<JobEvent>();
        let mut handles = Vec::with_capacity(workers);
        for i in 0..workers {
            let submit_rx = submit_rx.clone();
            let events_tx = events_tx.clone();
            let handle = thread::Builder::new()
                .name(format!("teatui-job-{i}"))
                .spawn(move || worker_loop(submit_rx, events_tx))
                .expect("failed to spawn worker thread");
            handles.push(handle);
        }
        Self {
            submit_tx,
            events_rx,
            next_id: Arc::new(AtomicU64::new(1)),
            _workers: handles,
        }
    }

    pub fn submitter(&self) -> JobSubmitter {
        JobSubmitter {
            submit_tx: self.submit_tx.clone(),
            next_id: self.next_id.clone(),
        }
    }

    pub fn events(&self) -> &Receiver<JobEvent> {
        &self.events_rx
    }
}

impl JobSubmitter {
    pub fn submit<J: Job>(&self, job: J) -> JobId {
        let id = JobId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let submission = Submission {
            id,
            job: Box::new(job),
        };
        if let Err(err) = self.submit_tx.send(submission) {
            tracing::error!(
                target: "teatui::jobs",
                name = err.0.job.name(),
                "submit failed — pool dead"
            );
        }
        id
    }
}

fn worker_loop(rx: Receiver<Submission>, tx: Sender<JobEvent>) {
    while let Ok(Submission { id, job }) = rx.recv() {
        let name = job.name();
        let started = Instant::now();
        tracing::debug!(target: "teatui::jobs", job_id = id.0, name, "start");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run_boxed()));

        let event = match outcome {
            Ok(JobOutcome::Done(any)) => JobEvent {
                id,
                name,
                outcome: JobOutcomeEvent::Done(any),
            },
            Ok(JobOutcome::Failed(msg)) => JobEvent {
                id,
                name,
                outcome: JobOutcomeEvent::Failed(msg),
            },
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "job panicked".to_string());
                tracing::error!(target: "teatui::jobs", job_id = id.0, name, %msg, "panic");
                JobEvent {
                    id,
                    name,
                    outcome: JobOutcomeEvent::Failed(msg),
                }
            }
        };

        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::debug!(target: "teatui::jobs", job_id = id.0, name, elapsed_ms, "done");

        if tx.send(event).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct EchoJob(u32);
    impl Job for EchoJob {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn run(self: Box<Self>) -> JobOutcome {
            JobOutcome::Done(Box::new(self.0))
        }
    }

    struct PanicJob;
    impl Job for PanicJob {
        fn name(&self) -> &'static str {
            "panic"
        }
        fn run(self: Box<Self>) -> JobOutcome {
            panic!("kaboom");
        }
    }

    #[test]
    fn submits_and_reports_done() {
        let jobs = Jobs::new(2);
        let id = jobs.submitter().submit(EchoJob(7));
        let event = jobs
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("job event");
        assert_eq!(event.id, id);
        match event.outcome {
            JobOutcomeEvent::Done(any) => {
                let v = any.downcast::<u32>().expect("u32");
                assert_eq!(*v, 7);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn worker_recovers_from_panic_and_reports_failure() {
        let jobs = Jobs::new(1);
        let submitter = jobs.submitter();
        let id = submitter.submit(PanicJob);
        let event = jobs
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("job event");
        assert_eq!(event.id, id);
        match event.outcome {
            JobOutcomeEvent::Failed(msg) => assert!(msg.contains("kaboom")),
            other => panic!("expected Failed, got {other:?}"),
        }
        let id2 = submitter.submit(EchoJob(1));
        let event2 = jobs
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("job event");
        assert_eq!(event2.id, id2);
    }

    #[test]
    fn submitter_is_cloneable_and_assigns_distinct_ids() {
        let jobs = Jobs::new(1);
        let a = jobs.submitter();
        let b = a.clone();
        let id1 = a.submit(EchoJob(1));
        let id2 = b.submit(EchoJob(2));
        assert_ne!(id1, id2);
        // Drain both events to keep the pool quiet.
        let _ = jobs.events().recv_timeout(Duration::from_secs(2));
        let _ = jobs.events().recv_timeout(Duration::from_secs(2));
    }
}
