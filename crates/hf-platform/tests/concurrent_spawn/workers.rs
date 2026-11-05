use super::support::{Child, deadline, spawn};
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

type Outcome = Result<Child, Box<dyn Any + Send>>;

fn start_worker(
    gate: Arc<(Mutex<bool>, Condvar)>,
    ready: Sender<()>,
    done: SyncSender<Outcome>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        ready.send(()).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let (lock, changed) = &*gate;
            let open = lock.lock().unwrap();
            let (open, timeout) = changed
                .wait_timeout_while(open, std::time::Duration::from_secs(2), |v| !*v)
                .unwrap();
            assert!(
                *open && !timeout.timed_out(),
                "concurrent launch gate expired"
            );
            drop(open);
            let child = Child::new(spawn(&["hold"]));
            assert_eq!(
                child.0.initialization(),
                hf_platform::process::SpawnInitialization::Ready,
                "concurrent owned child failed identity qualification"
            );
            child
        }));
        done.send(result).unwrap();
    })
}

fn collect_workers(workers: Vec<JoinHandle<()>>, done: Receiver<Outcome>) -> Vec<Child> {
    let mut results = Vec::new();
    let end = deadline(5_000);
    for _ in 0..4 {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            std::process::abort();
        }
        match done.recv_timeout(remaining) {
            Ok(result) => results.push(result),
            Err(_) => std::process::abort(),
        }
    }
    let settle_end = deadline(1_000);
    while workers.iter().any(|worker| !worker.is_finished()) {
        if Instant::now() >= settle_end {
            std::process::abort();
        }
        std::hint::spin_loop();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    results.into_iter().map(Result::unwrap).collect()
}

pub(super) fn launch_concurrently() -> Vec<Child> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::sync_channel(4);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let workers = (0..4)
        .map(|_| start_worker(gate.clone(), ready_tx.clone(), done_tx.clone()))
        .collect();
    drop(ready_tx);
    let ready_end = deadline(2_000);
    for _ in 0..4 {
        let remaining = ready_end.saturating_duration_since(Instant::now());
        if remaining.is_zero() || ready_rx.recv_timeout(remaining).is_err() {
            std::process::abort();
        }
    }
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    drop(done_tx);
    collect_workers(workers, done_rx)
}
