//! The 4 MiB prompt store for the `task-prompt` component case.
use std::path::Path;

/// `sidecar`: a preview plus a digest-verified stored full text, the form long prompts take.
/// `inline`: the whole text in the inline column (the reader limit refuses it).
pub fn generate(root: &Path, inline: bool) {
    let mut rng = super::data::Rng::new(super::data::SEED);
    let mut task = super::data::valid_tasks(1, &mut rng).remove(0);
    let text = "p".repeat(4 * 1024 * 1024);
    task.prompt = if inline {
        hf_storage::FixturePrompt {
            inline_text: Some(text),
            ..task.prompt
        }
    } else {
        let mut hash = hf_storage::Sha256Stream::with_limit(text.len()).unwrap();
        hash.update(text.as_bytes()).unwrap();
        let digest = hash.finish().unwrap().as_str().to_owned();
        hf_storage::FixturePrompt {
            inline_text: None,
            preview: Some(text.chars().take(2000).collect()),
            source_name: Some(format!("t-00000000.prompt.{digest}.txt")),
            claimed_sha256: Some(digest),
            blob: Some(text.into_bytes()),
        }
    };
    let deadline = || std::time::Instant::now() + std::time::Duration::from_secs(60);
    let store = hf_storage::Store::initialize_fixture(
        root,
        super::data::limits(),
        deadline(),
        super::common::lease::lease_port(),
    )
    .unwrap();
    let all = hf_domain::task_read::Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let empty = hf_storage::TaskFixture {
        rows: Vec::new(),
        launches: Vec::new(),
        coverage: all,
    };
    store.replace_task_fixture(&empty, deadline()).unwrap();
    store.add_task_fixture(&task, deadline()).unwrap();
}
