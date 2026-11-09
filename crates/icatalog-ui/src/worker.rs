//! Background command execution; all catalogue behaviour stays in the engine.
use icatalog_engine::{dispatch_guarded, Context, EngineError, Outcome, Params, Result};
use std::path::PathBuf;
use std::thread::JoinHandle;

pub struct Worker {
    handle: Option<JoinHandle<Result<Outcome>>>,
}

impl Worker {
    pub fn start(path: PathBuf, id: String, params: Params) -> std::io::Result<Self> {
        let handle = std::thread::Builder::new()
            .name("icatalog-command".into())
            .spawn(move || {
                let mut context = if id == "clone" {
                    Context::for_destination(path)?
                } else {
                    Context::open(path)?
                };
                if icatalog_engine::find_command(&id).is_some() {
                    dispatch_guarded(&mut context, &id, &params)
                } else {
                    icatalog_sync::dispatch_guarded(&mut context, &id, &params)
                }
            })?;
        Ok(Self {
            handle: Some(handle),
        })
    }

    pub fn poll(&mut self) -> Option<Result<Outcome>> {
        if !self.handle.as_ref()?.is_finished() {
            return None;
        }
        Some(match self.handle.take()?.join() {
            Ok(result) => result,
            Err(_) => Err(EngineError::Internal("command worker panicked".into())),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn background_command_reports_success_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker.sqlite");
        for (id, success) in [("init", true), ("missing-command", false)] {
            let mut worker = Worker::start(path.clone(), id.into(), Params::new()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Some(result) = worker.poll() {
                    assert_eq!(result.is_ok(), success);
                    assert!(worker.poll().is_none());
                    break;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}
