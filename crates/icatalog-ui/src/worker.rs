//! Background command execution; all catalogue behaviour stays in the engine.
use icatalog_engine::{dispatch_guarded, Context, EngineError, Outcome, Params, Result};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

pub struct Worker {
    handle: Option<JoinHandle<Result<Outcome>>>,
    progress: std::sync::Arc<std::sync::Mutex<Option<icatalog_engine::SyncProgress>>>,
}

#[derive(Clone, Default)]
struct LatestProgress(std::sync::Arc<std::sync::Mutex<Option<icatalog_engine::SyncProgress>>>);

impl LatestProgress {
    fn store(&self, progress: icatalog_engine::SyncProgress) {
        let mut latest = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *latest = Some(progress);
    }

    fn take(&self) -> Option<icatalog_engine::SyncProgress> {
        let mut latest = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        latest.take()
    }
}

pub(crate) fn open_initialized_existing(path: &Path) -> Result<Context> {
    if std::fs::metadata(path)?.len() == 0 {
        return Err(EngineError::BadParam(
            "catalogue file is empty; restore a backup or initialise a new file".into(),
        ));
    }
    let context = Context::open_existing(path)?;
    if context.catalog().catalogue()?.is_none() {
        return Err(EngineError::BadParam(
            "catalogue is not initialised; restore a backup or initialise a new file".into(),
        ));
    }
    Ok(context)
}

impl Worker {
    #[cfg(test)]
    pub(crate) fn blocked_for_test(release: std::sync::mpsc::Receiver<()>) -> Self {
        Self {
            handle: Some(std::thread::spawn(move || {
                let _ = release.recv();
                Ok(Outcome::Text("test worker finished".into()))
            })),
            progress: LatestProgress::default().0,
        }
    }

    pub fn start(path: PathBuf, id: String, params: Params) -> std::io::Result<Self> {
        let progress = LatestProgress::default();
        let progress_sink = progress.clone();
        let handle = std::thread::Builder::new()
            .name("icatalog-command".into())
            .spawn(move || {
                let read_only = matches!(id.as_str(), "sync-check")
                    || (id == "sync-resolve"
                        && matches!(params.get("action"), Some("list" | "show")));
                let mut context = match id.as_str() {
                    "clone" => Context::for_destination(path)?,
                    "init" => Context::open(path)?,
                    _ if read_only => Context::open_read_only(&path)?,
                    _ => open_initialized_existing(&path)?,
                };
                context.set_progress_sink(Some(std::sync::Arc::new(move |event| {
                    progress_sink.store(event);
                })));
                if icatalog_engine::find_command(&id).is_some() {
                    dispatch_guarded(&mut context, &id, &params)
                } else {
                    icatalog_sync::dispatch_guarded(&mut context, &id, &params)
                }
            })?;
        Ok(Self {
            handle: Some(handle),
            progress: progress.0,
        })
    }

    pub fn take_latest_progress(&self) -> Option<icatalog_engine::SyncProgress> {
        LatestProgress(self.progress.clone()).take()
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
    fn progress_storage_keeps_only_the_latest_bounded_event() {
        let progress = LatestProgress::default();
        progress.store(icatalog_engine::SyncProgress {
            phase: icatalog_engine::SyncPhase::Downloading,
            completed: Some(32),
            total: None,
        });
        progress.store(icatalog_engine::SyncProgress {
            phase: icatalog_engine::SyncPhase::Validating,
            completed: Some(1),
            total: Some(1),
        });
        assert_eq!(
            progress.take(),
            Some(icatalog_engine::SyncProgress {
                phase: icatalog_engine::SyncPhase::Validating,
                completed: Some(1),
                total: Some(1),
            })
        );
        assert_eq!(progress.take(), None);
    }

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

    #[test]
    fn sync_worker_does_not_create_a_missing_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.sqlite");
        let mut worker = Worker::start(path.clone(), "sync".into(), Params::new()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = worker.poll() {
                assert!(result.is_err());
                assert!(!path.exists());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
