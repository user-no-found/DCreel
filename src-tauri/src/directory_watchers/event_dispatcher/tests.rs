use super::EventDispatcher;
use std::{
    collections::HashSet,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn callbacks_continue_and_changes_coalesce_while_dispatch_is_blocked() {
    let (sent, received) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let mut first = true;
    let dispatcher = EventDispatcher::new(move |id| {
        sent.send(id.to_string()).unwrap();
        if first {
            first = false;
            wait_release.recv_timeout(TIMEOUT).unwrap();
        }
    })
    .unwrap();
    let sink = dispatcher.sink();
    sink.enqueue("busy");
    assert_eq!(received.recv_timeout(TIMEOUT).unwrap(), "busy");
    let (enqueued, done) = mpsc::channel();
    thread::spawn(move || {
        for _ in 0..10_000 {
            assert!(sink.enqueue("changed"));
        }
        enqueued.send(()).unwrap();
    });
    let completed = done.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    completed.expect("notify callback waited for dispatch IO");
    assert_eq!(received.recv_timeout(TIMEOUT).unwrap(), "changed");
    assert!(received.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn a_full_wake_channel_does_not_drop_distinct_fences() {
    let (sent, received) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let dispatcher = EventDispatcher::new(move |id| {
        sent.send(id.to_string()).unwrap();
        if id == "busy" {
            wait_release.recv_timeout(TIMEOUT).unwrap();
        }
    })
    .unwrap();
    let sink = dispatcher.sink();
    sink.enqueue("busy");
    assert_eq!(received.recv_timeout(TIMEOUT).unwrap(), "busy");
    for _ in 0..100 {
        sink.enqueue("first");
        sink.enqueue("second");
    }
    release.send(()).unwrap();
    let ids: HashSet<_> = (0..2)
        .map(|_| received.recv_timeout(TIMEOUT).unwrap())
        .collect();
    assert_eq!(ids, HashSet::from(["first".into(), "second".into()]));
}

#[test]
fn shutdown_does_not_wait_for_io_or_dispatch_queued_events() {
    let (sent, received) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let dispatcher = EventDispatcher::new(move |id| {
        sent.send(id.to_string()).unwrap();
        wait_release.recv_timeout(TIMEOUT).unwrap();
    })
    .unwrap();
    let sink = dispatcher.sink();
    sink.enqueue("busy");
    received.recv_timeout(TIMEOUT).unwrap();
    sink.enqueue("queued");
    let (stopped, done) = mpsc::channel();
    thread::spawn(move || {
        drop(dispatcher);
        stopped.send(()).unwrap();
    });
    let completed = done.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    completed.expect("UI shutdown waited for background IO");
    assert!(!sink.enqueue("after-shutdown"));
    assert!(received.recv_timeout(TIMEOUT).is_err());
}

#[test]
fn dispatcher_drop_wakes_an_idle_worker() {
    let dispatcher = EventDispatcher::new(|_| {}).unwrap();
    let sink = dispatcher.sink();
    dispatcher.stop();
    let deadline = Instant::now() + TIMEOUT;
    while !dispatcher.worker.as_ref().unwrap().is_finished() {
        assert!(
            Instant::now() < deadline,
            "idle worker retained its app handle"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!sink.enqueue("closed"));
}

#[cfg(windows)]
#[test]
fn windows_notify_callbacks_continue_during_alertable_dispatch_io() {
    use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    unsafe extern "system" {
        fn SleepEx(milliseconds: u32, alertable: i32) -> u32;
    }

    let directory = std::env::temp_dir().join(format!("dcreel-watcher-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let (started, start) = mpsc::channel();
    let (delivered, delivery) = mpsc::channel();
    let dispatcher = EventDispatcher::new(move |id| {
        started.send(thread::current().id()).unwrap();
        // A host pipe write uses an alertable wait. It belongs on this worker,
        // away from the thread receiving ReadDirectoryChangesW callbacks.
        unsafe { SleepEx(150, 1) };
        delivered.send(id.to_string()).unwrap();
    })
    .unwrap();
    let sink = dispatcher.sink();
    let (callbacks, callback) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && !matches!(event.kind, EventKind::Access(_))
            {
                sink.enqueue("fixture");
                let _ = callbacks.send((thread::current().id(), event.paths));
            }
        },
        Config::default(),
    )
    .unwrap();
    watcher
        .watch(&directory, RecursiveMode::NonRecursive)
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        std::fs::write(directory.join("first.txt"), "first").unwrap();
        let worker = start.recv_timeout(TIMEOUT).unwrap();
        for index in 0..32 {
            std::fs::write(directory.join(format!("burst-{index}.txt")), "changed").unwrap();
        }
        let deadline = Instant::now() + TIMEOUT;
        let mut paths = HashSet::new();
        while paths.len() < 33 {
            let (callback_thread, changed) = callback
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            assert_ne!(callback_thread, worker);
            paths.extend(changed);
        }
        assert_eq!(delivery.recv_timeout(TIMEOUT).unwrap(), "fixture");
    });
    dispatcher.stop();
    drop(watcher);
    while !dispatcher.worker.as_ref().unwrap().is_finished() {
        thread::sleep(Duration::from_millis(5));
    }
    drop(dispatcher);
    std::fs::remove_dir_all(directory).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
