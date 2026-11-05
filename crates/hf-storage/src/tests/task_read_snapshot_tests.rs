#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn task_read_snapshot_child() {
    let Ok(root) = std::env::var("HF_STORAGE_STAGE_ROOT") else {
        return;
    };
    *STAGE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some("task_read_snapshot".to_owned());
    let store = Store::open(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(10),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let query = TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap();
    let result = store
        .task_read(
            &AuthorizedScope::Global,
            &query,
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let names = result
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    println!("TASK_READ_IDS:{names}");
}

#[test]
fn task_read_uses_one_sqlite_snapshot_while_a_fixture_is_replaced() {
    let _guard = fixture_guard();
    let parent =
        std::env::temp_dir().join(format!("hf-storage-read-snapshot-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let writer = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![snapshot_row("old-task")],
                pending: Vec::new(),
                coverage: snapshot_coverage(),
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();

    let (mut child, receiver) = spawn_snapshot_child(&root);
    wait_for_stage(&receiver, "task_read_snapshot");
    writer
        .replace_task_fixture(
            &TaskFixture {
                rows: vec![snapshot_row("new-task")],
                pending: Vec::new(),
                coverage: snapshot_coverage(),
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    child.0.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(child.0.wait().unwrap().success());

    let observed = collect_output(receiver);
    assert!(
        observed.iter().any(|line| line == "TASK_READ_IDS:old-task"),
        "child output: {observed:?}"
    );
    let fresh = writer
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap(),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(fresh.rows[0].id.as_str(), "new-task");
    drop(writer);
    fs::remove_dir_all(parent).unwrap();
}

fn spawn_snapshot_child(root: &std::path::Path) -> (StageChild, mpsc::Receiver<Option<String>>) {
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::task_read_snapshot_tests::task_read_snapshot_child",
            "--nocapture",
        ])
        .env("HF_STORAGE_STAGE_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = StageChild(child);
    let output = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if sender.send(line.ok()).is_err() {
                return;
            }
        }
        let _ = sender.send(None);
    });
    (child, receiver)
}

fn wait_for_stage(receiver: &mpsc::Receiver<Option<String>>, stage: &str) {
    for _ in 0..32 {
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("snapshot child output timeout")
            .expect("snapshot child exited before its read snapshot");
        if line.trim() == format!("STAGE:{stage}") {
            return;
        }
    }
    panic!("task-read snapshot boundary was not reached");
}

fn collect_output(receiver: mpsc::Receiver<Option<String>>) -> Vec<String> {
    let mut observed = Vec::new();
    while let Some(line) = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("snapshot child output timeout")
    {
        observed.push(line);
    }
    observed
}
