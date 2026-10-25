//! A fake Claude Code for tests: it answers each call with a function of the call, never starting anything, and
//! keeps the calls it got so a test can check what Vacancy would have sent.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::runner::{Call, Cli, Control, Failure, Progress, Reply};

type Answerer = Box<dyn Fn(&Call) -> Result<Reply, Failure> + Send + Sync>;

/// Answers with `answer`, after `delay` (watching cancel, the timeout and the slow note like the real call).
pub struct FakeClaude {
    answer: Answerer,
    pub delay: Duration,
    calls: Mutex<Vec<Call>>,
}

impl std::fmt::Debug for FakeClaude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeClaude")
            .field("delay", &self.delay)
            .finish_non_exhaustive()
    }
}

impl FakeClaude {
    pub fn new(answer: impl Fn(&Call) -> Result<Reply, Failure> + Send + Sync + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            delay: Duration::ZERO,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Always answers `structured`, with nothing refused and no model named.
    pub fn always(structured: serde_json::Value) -> Self {
        Self::new(move |_| {
            Ok(Reply {
                structured: structured.clone(),
                refused: Vec::new(),
                model: None,
            })
        })
    }

    /// Every call it got, oldest first.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl Cli for FakeClaude {
    fn call(
        &self,
        call: &Call,
        control: &Control,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Reply, Failure> {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(call.clone());
        let started = Instant::now();
        let mut slow = false;
        while started.elapsed() < self.delay {
            if control.cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            if started.elapsed() >= control.timeout {
                return Err(Failure::TimedOut);
            }
            if !slow && started.elapsed() >= control.slow_after {
                slow = true;
                progress(Progress::Slow);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if control.cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        (self.answer)(call)
    }
}
