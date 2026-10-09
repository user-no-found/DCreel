use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

#[derive(Default)]
struct PendingEvents {
    ids: HashSet<String>,
    stopped: bool,
}

#[derive(Clone)]
pub(super) struct EventSink {
    pending: Arc<Mutex<PendingEvents>>,
    wake: SyncSender<()>,
}

impl EventSink {
    // notify holds its handler mutex on a Windows completion callback. This path
    // must not perform IO or an alertable wait that could reenter that callback.
    pub(super) fn enqueue(&self, id: &str) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        if pending.stopped {
            return false;
        }
        pending.ids.insert(id.into());
        drop(pending);
        // A full wake channel already guarantees that the pending set is read.
        let _ = self.wake.try_send(());
        true
    }
}

pub(super) struct EventDispatcher {
    sink: EventSink,
    worker: Option<JoinHandle<()>>,
}

impl EventDispatcher {
    pub(super) fn new(mut dispatch: impl FnMut(&str) + Send + 'static) -> Result<Self, String> {
        let pending = Arc::new(Mutex::new(PendingEvents::default()));
        let (wake, receiver) = mpsc::sync_channel(1);
        let worker_pending = Arc::clone(&pending);
        let worker = thread::Builder::new()
            .name("directory-event-dispatcher".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    let ids = {
                        let Ok(mut pending) = worker_pending.lock() else {
                            log::error!(target: "directory_watcher", "directory event queue is poisoned");
                            return;
                        };
                        if pending.stopped {
                            return;
                        }
                        std::mem::take(&mut pending.ids)
                    };
                    for id in ids {
                        if worker_pending.lock().map_or(true, |pending| pending.stopped) {
                            return;
                        }
                        // Neither the queue lock nor notify's callback mutex is
                        // held while host IPC or WebView event delivery runs.
                        dispatch(&id);
                    }
                }
            })
            .map_err(|error| format!("无法启动目录事件处理线程：{error}"))?;
        Ok(Self {
            sink: EventSink { pending, wake },
            worker: Some(worker),
        })
    }

    pub(super) fn sink(&self) -> EventSink {
        self.sink.clone()
    }

    pub(super) fn stop(&self) {
        if let Ok(mut pending) = self.sink.pending.lock() {
            pending.stopped = true;
            pending.ids.clear();
        }
        let _ = self.sink.wake.try_send(());
    }
}

impl Drop for EventDispatcher {
    fn drop(&mut self) {
        self.stop();
        // In-flight IO finishes on the worker; shutdown must not wait for it on
        // the UI thread. Finished workers can be joined without blocking.
        if let Some(worker) = self.worker.take().filter(JoinHandle::is_finished)
            && worker.join().is_err()
        {
            log::error!(target: "directory_watcher", "directory event worker panicked");
        }
    }
}

#[cfg(test)]
mod tests;
