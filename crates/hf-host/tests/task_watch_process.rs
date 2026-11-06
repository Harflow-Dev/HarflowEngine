#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "task_watch_process/mod.rs"]
mod support;
use hf_application::task_watch::TaskWatchPort;
use hf_domain::task_watch::{WatchRequest, WatchState};
use hf_interfaces::watch_codec::WatchJsonCodec;
use hf_storage::{Limits, Store, WatchSession};
use std::{
    io::{BufRead, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use support::*;
#[test]
fn another_process_commits_between_watch_snapshots_and_result_is_changed() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let initial = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    drop(initial);
    let baseline = WatchSession::new(
        &root,
        Limits::default(),
        WatchJsonCodec,
        hf_storage::RealWatchControl::default(),
        hf_platform::lease::shared(),
    )
    .execute(
        &WatchRequest {
            cursor: None,
            wait_seconds: 0,
        },
        deadline(),
    )
    .unwrap()
    .cursor
    .unwrap();
    let mut cursor = baseline;
    for _ in 0..2 {
        let mut actor = ProcessGuard::spawn(&root);
        actor.wait_for("WATCH_ACTOR_READY");
        let (send_entered, entered) = mpsc::channel();
        let control = PollControl::default();
        *control.0.entered.lock().unwrap() = Some(send_entered);
        let request = WatchRequest {
            cursor: Some(cursor),
            wait_seconds: 4,
        };
        let watched_root = root.clone();
        let (send_result, received_result) = mpsc::channel();
        let watcher = thread::spawn(move || {
            let result = WatchSession::new(
                watched_root,
                Limits::default(),
                WatchJsonCodec,
                control,
                hf_platform::lease::shared(),
            )
            .execute(&request, deadline());
            let _ = send_result.send(result);
        });
        entered
            .recv_timeout(Duration::from_secs(5))
            .expect("watch must reach bounded wait");
        actor.go();
        actor.wait_for("WATCH_ACTOR_COMMITTED");
        let result = received_result
            .recv_timeout(Duration::from_secs(6))
            .expect("watch result deadline")
            .unwrap();
        wait_thread_exit(&watcher, Instant::now() + Duration::from_secs(2));
        watcher.join().unwrap();
        assert_eq!(result.state, WatchState::Changed);
        cursor = result
            .cursor
            .expect("changed result retains the newly observed cursor");
        actor.wait_success();
    }
}

fn wait_thread_exit(handle: &thread::JoinHandle<()>, deadline: Instant) {
    while !handle.is_finished() && Instant::now() < deadline {
        thread::yield_now();
    }
    assert!(
        handle.is_finished(),
        "watch worker sent a result but did not exit within the cleanup deadline"
    );
}

#[test]
fn task_watch_writer_actor() {
    let Ok(root) = std::env::var("HF_WATCH_ACTOR_ROOT") else {
        return;
    };
    let _guard = support::FIXTURE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let store = Store::open(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    println!("WATCH_ACTOR_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "GO");
    store
        .replace_task_fixture(&task_fixture(), deadline())
        .unwrap();
    println!("WATCH_ACTOR_COMMITTED");
    std::io::stdout().flush().unwrap();
}
