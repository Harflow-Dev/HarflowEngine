//! In-process throughput: one long-lived writer handle per task on one root. Each task
//! replays a schedule of event lines; whenever lines are queued, one capture-publication
//! commit and one append commit drain everything queued for that task (adaptive batching).
//! The lag of a line is the time from its scheduled arrival to the return of its commit.
//! `HF_WL_CAPTURE=0` leaves out the capture-publication commit (a diagnostic run).
use super::support::{
    self, WorkDir, batch, copy_tree, event, micros, open, out_file, outcome, retry, task, task_id,
};
use hf_domain::task_read::TaskId;
use hf_storage::Store;
use std::{
    fs::File,
    io::Write,
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

/// How long the parent waits for every task to finish its setup.
const READY_DEADLINE: Duration = Duration::from_secs(60);

/// One scheduled line: arrival in microseconds from the start, raw length, repetition.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub at_us: u64,
    pub len: usize,
    pub rep: u64,
}

/// A profile file: one `t_ms len` pair per line, as extracted from a recording timeline,
/// sorted by time.
pub fn read_profile(path: &Path) -> Vec<(u64, usize)> {
    let mut profile: Vec<(u64, usize)> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut parts = l.split_whitespace();
            let t: u64 = parts.next().unwrap().parse().unwrap();
            (t, parts.next().unwrap().parse().unwrap())
        })
        .collect();
    profile.sort_by_key(|p| p.0);
    profile
}

/// The recording replayed `reps` times, each repetition starting `gap_ms` after the
/// previous one's last line, with the first line of each repetition at its offset.
pub fn burst(profile: &[(u64, usize)], reps: u64, gap_ms: u64) -> Vec<Line> {
    let first = profile.first().map_or(0, |p| p.0);
    let span = profile.last().map_or(0, |p| p.0).saturating_sub(first);
    (0..reps)
        .flat_map(|rep| {
            let base = rep * (span + gap_ms);
            profile.iter().map(move |&(t, len)| Line {
                at_us: (base + t.saturating_sub(first)) * 1000,
                len,
                rep,
            })
        })
        .collect()
}

/// `rate` lines per second for `seconds`, evenly spaced from `offset_us`, sizes cycled
/// from `sizes`.
pub fn sustained(sizes: &[usize], rate: u64, seconds: u64, offset_us: u64) -> Vec<Line> {
    (0..rate * seconds)
        .map(|n| Line {
            at_us: offset_us + n * 1_000_000 / rate,
            len: sizes[n as usize % sizes.len()],
            rep: 0,
        })
        .collect()
}

#[derive(Default)]
struct Totals {
    commits: u64,
    busy: u64,
    errors: u64,
}

/// One task's writer handle, its two tasks and its raw output, set up before the start.
struct Worker {
    store: Store,
    main: TaskId,
    cap: TaskId,
    file: File,
    totals: Totals,
}

fn setup(name: &str, root: &Path, mode: &str, k: usize) -> Result<Worker, String> {
    let mut totals = Totals::default();
    let store =
        open(root, mode, "suppressed", &mut totals.busy).map_err(|e| format!("open: {e:?}"))?;
    let (main, cap) = (format!("{name}-t{k}"), format!("{name}-c{k}"));
    for id in [&main, &cap] {
        retry(&mut totals.busy, || {
            store.create_task(&task(id), support::later(10))
        })
        .map_err(|e| format!("create {id}: {e:?}"))?;
    }
    let file = out_file(&format!("{name}-t{k}.jsonl"));
    Ok(Worker {
        store,
        main: task_id(&main),
        cap: task_id(&cap),
        file,
        totals,
    })
}

/// Sets up, reports readiness, waits for the parent's go (or abort), then replays.
fn worker(
    name: &str,
    root: &Path,
    mode: &str,
    k: usize,
    lines: &[Line],
    ready: &Sender<Result<(), String>>,
    go: &Receiver<bool>,
) -> Option<Totals> {
    let set = setup(name, root, mode, k);
    let _ = ready.send(set.as_ref().map(|_| ()).map_err(Clone::clone));
    let worker = set.ok()?;
    go.recv().ok().filter(|&go| go)?;
    Some(replay(worker, k, lines))
}

/// One task's replay on its own handle. Writes one raw line per event line.
fn replay(mut w: Worker, k: usize, lines: &[Line]) -> Totals {
    let capture = support::env_or("HF_WL_CAPTURE", "1") == "1";
    let began = Instant::now();
    let (mut next, mut seq) = (0usize, 1u64);
    while next < lines.len() {
        let now = micros(began);
        let ready = lines[next..].iter().take_while(|l| l.at_us <= now).count();
        if ready == 0 {
            let wait = lines[next].at_us.saturating_sub(now);
            std::thread::sleep(Duration::from_micros(wait.min(1000)));
            continue;
        }
        let proxy = [event(seq, &"c".repeat(200))];
        let published = if capture {
            w.totals.commits += 1;
            retry(&mut w.totals.busy, || {
                w.store
                    .append_events(&w.cap, &proxy, &support::facts(), support::later(10))
            })
        } else {
            Ok(())
        };
        let queued = &lines[next..next + ready];
        let appended: Vec<_> = queued
            .iter()
            .enumerate()
            .map(|(n, l)| event(seq + n as u64, &"x".repeat(l.len.max(1))))
            .collect();
        let result = retry(&mut w.totals.busy, || {
            w.store
                .append_events(&w.main, &appended, &support::facts(), support::later(10))
        });
        let done = micros(began);
        w.totals.commits += 1;
        w.totals.errors += u64::from(published.is_err()) + u64::from(result.is_err());
        for l in queued {
            let lag = done - l.at_us;
            writeln!(
                w.file,
                r#"{{"task":{k},"rep":{},"at_us":{},"lag_us":{lag},"batch":{ready},"err":"{}"}}"#,
                l.rep,
                l.at_us,
                outcome(&result)
            )
            .unwrap();
        }
        seq += ready as u64;
        next += ready;
    }
    w.totals
}

/// Waits for every worker's readiness; the setup errors, or a missing report, if any.
fn await_ready(ready: &Receiver<Result<(), String>>, count: usize) -> Vec<String> {
    let until = Instant::now() + READY_DEADLINE;
    (0..count)
        .filter_map(|_| {
            let left = until.saturating_duration_since(Instant::now());
            match ready.recv_timeout(left) {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(format!("no readiness report: {error}")),
            }
        })
        .collect()
}

/// Four concurrent tasks replaying their schedules on one copy of the dataset root. A
/// task whose setup fails aborts the run before any replay starts.
pub fn run(name: &str, mode: &str, schedules: Vec<Vec<Line>>) {
    let work = WorkDir::new(&format!("wl-{name}"));
    let source = support::dir().join(support::env_or("HF_MEASURE_CASE", "ci-ckpt"));
    copy_tree(&source, &work.0);
    let count = schedules.len();
    let (ready_tx, ready_rx) = mpsc::channel();
    batch(name, "start");
    let (gos, threads): (Vec<_>, Vec<_>) = schedules
        .into_iter()
        .enumerate()
        .map(|(k, lines)| {
            let (go_tx, go_rx) = mpsc::channel();
            let (name, mode, root, ready) = (
                name.to_owned(),
                mode.to_owned(),
                work.0.clone(),
                ready_tx.clone(),
            );
            let thread =
                std::thread::spawn(move || worker(&name, &root, &mode, k, &lines, &ready, &go_rx));
            (go_tx, thread)
        })
        .unzip();
    let failed = await_ready(&ready_rx, count);
    for go in &gos {
        let _ = go.send(failed.is_empty());
    }
    let began = Instant::now();
    let totals: Vec<Option<Totals>> = threads
        .into_iter()
        .map(|t| t.join().ok().flatten())
        .collect();
    let seconds = began.elapsed().as_secs_f64();
    batch(name, "end");
    assert!(failed.is_empty(), "throughput setup failed: {failed:?}");
    let totals: Vec<Totals> = totals
        .into_iter()
        .map(|t| t.expect("a replay task failed"))
        .collect();
    let commits: u64 = totals.iter().map(|t| t.commits).sum();
    let busy: u64 = totals.iter().map(|t| t.busy).sum();
    let errors: u64 = totals.iter().map(|t| t.errors).sum();
    writeln!(out_file("throughput.jsonl"), r#"{{"run":"{name}","mode":"{mode}","seconds":{seconds:.3},"commits":{commits},"commits_per_s":{:.1},"busy":{busy},"errors":{errors}}}"#, commits as f64 / seconds).unwrap();
}

/// The commit rate of one writer appending one 1 KiB line per commit for `seconds`.
pub fn max_rate(name: &str, mode: &str, seconds: u64) {
    let work = WorkDir::new(&format!("wl-{name}"));
    let source = support::dir().join(support::env_or("HF_MEASURE_CASE", "ci-ckpt"));
    copy_tree(&source, &work.0);
    let mut busy = 0;
    let store = open(&work.0, mode, "suppressed", &mut busy).unwrap();
    store.create_task(&task(name), support::later(10)).unwrap();
    let id = task_id(name);
    let mut file = out_file(&format!("{name}.jsonl"));
    batch(name, "start");
    let began = Instant::now();
    let (mut seq, mut errors) = (1u64, 0u64);
    while began.elapsed() < Duration::from_secs(seconds) {
        let line = support::events(seq, 1, 1024);
        let started = Instant::now();
        let result = retry(&mut busy, || {
            store.append_events(&id, &line, &support::facts(), support::later(10))
        });
        errors += u64::from(result.is_err());
        writeln!(file, r#"{{"k":{seq},"wall_us":{}}}"#, micros(started)).unwrap();
        seq += 1;
    }
    let elapsed = began.elapsed().as_secs_f64();
    batch(name, "end");
    let commits = seq - 1;
    writeln!(out_file("throughput.jsonl"), r#"{{"run":"{name}","mode":"{mode}","seconds":{elapsed:.3},"commits":{commits},"commits_per_s":{:.1},"busy":{busy},"errors":{errors}}}"#, commits as f64 / elapsed).unwrap();
    drop(store);
}
