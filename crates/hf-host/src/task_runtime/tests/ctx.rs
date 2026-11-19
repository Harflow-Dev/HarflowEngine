//! Scratch area, scripted caller chain and seeded launches.
use super::{
    agents::{Fake, Gates},
    caller_source::{Chain, Scripted},
};
use super::{
    cleanup::Cleanup,
    harness::{deadline, id, leases},
    probe::{Probe, Seed},
};
use crate::task_runtime::{HeldRoot, store_options};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    ownership::PreparedRequest,
    task_launch::CliSessionId,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_interfaces::task_runtime::{
    InitialLaunchMetadata, encode_environment, encode_invitation, encode_metadata,
};
use hf_platform::backend_launch::{observe_binary_pin, observe_selection};
use hf_storage::{Limits, RootHold, Store};
use std::{
    cell::RefCell, ffi::OsString, os::unix::fs::MetadataExt, path::PathBuf, rc::Rc, sync::Arc,
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// Scratch area, scripted caller chain and the fake agent of one case.
pub(super) struct Ctx {
    pub(super) cleanup: Rc<Cleanup>,
    pub(super) scripted: Arc<Scripted>,
    pub(super) chain: Chain,
    pub(super) fake: Fake,
    pub(super) dir: PathBuf,
    _scratch: Scratch,
}
impl Ctx {
    pub(super) fn new(name: &str) -> Self {
        let parent = std::env::temp_dir().join(format!("hf-s2-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir(&parent).unwrap();
        let dir = std::fs::canonicalize(&parent).unwrap();
        let scripted = Scripted::new(std::fs::metadata(&dir).unwrap().uid());
        let fake = Fake::install(&dir);
        let gates = Gates::new(&fake, &["a", "b", "c", "desc"]);
        Self {
            cleanup: Rc::new(Cleanup {
                gates: RefCell::new(gates),
                descendants: RefCell::new(Vec::new()),
            }),
            chain: scripted.chain(),
            scripted,
            fake,
            _scratch: Scratch(dir.clone()),
            dir,
        }
    }
    pub(super) fn root(&self) -> PathBuf {
        self.dir.join("store")
    }
    /// A fresh initialized root and its private T0 for one launch.
    pub(super) fn seed(&self, steps: &str, version: &str) -> Seed {
        self.seed_prompt(steps, version, "Run the scripted task.")
    }
    /// As `seed`, with the prompt the agent is sent.
    pub(super) fn seed_prompt(&self, steps: &str, version: &str, prompt: &str) -> Seed {
        let root = self.root();
        drop(
            Store::initialize_runtime(
                &root,
                Limits::default(),
                deadline(),
                leases(),
                store_options::suppressed(),
            )
            .unwrap(),
        );
        let source = RootHold::open(&root, deadline(), leases())
            .unwrap()
            .source_stamp()
            .clone();
        let env = self.fake.environment(steps, version);
        let seed = self.build(root, source, env, prompt.to_owned());
        let hold = HeldRoot::open(&seed.root, &seed.invitation, deadline()).unwrap();
        crate::task_runtime::prepare(&hold, &seed.preparation, &seed.intent, deadline()).unwrap();
        seed
    }
    fn build(
        &self,
        root: PathBuf,
        source: hf_domain::recovery::SourceStamp,
        env: Vec<(String, String)>,
        prompt: String,
    ) -> Seed {
        let session = CliSessionId::new("123e4567-e89b-42d3-a456-426614174000".into()).unwrap();
        let task = TaskId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()).unwrap();
        let env_os: Vec<_> = env
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();
        let hash = hf_storage::Sha256Digest;
        let backend = observe_selection(&env_os, &hash, deadline()).unwrap();
        let ctl = observe_binary_pin(&self.fake.bin.join("ctl-stub"), &hash, deadline()).unwrap();
        let cwd = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        let metadata = encode_metadata(&InitialLaunchMetadata {
            task: task.clone(),
            session: session.clone(),
            repo: cwd.to_str().unwrap().into(),
            title: None,
            model: None,
            created_at: crate::task_runtime::timestamp().unwrap(),
            caller_reference: format!(
                "{}:{}:{}:{}",
                self.chain.caller.pid(),
                self.chain.caller.start_seconds(),
                self.chain.caller.start_microseconds(),
                self.chain.caller.boot_uuid()
            ),
            configuration_reference: crate::task_runtime::configuration_digest().as_str().into(),
        })
        .unwrap();
        let binding = LifecycleBinding {
            task,
            operation: id(1),
            attempt: id(2),
            generation: source.generation.clone(),
        };
        let invitation = ProductionInvitation {
            binding: binding.clone(),
            reservation: id(4),
            ownership_session: id(5),
            session: session.clone(),
            source,
            caller: self.chain.caller.clone(),
            dispatcher: self.chain.dispatcher.clone(),
            ctl,
            backend,
            cwd: cwd.to_str().unwrap().into(),
            environment_digest: hash.digest_parts(&[&encode_environment(&env).unwrap()]),
            prompt_digest: hash.digest_parts(&[prompt.as_bytes()]),
            metadata_digest: hash.digest_parts(&[&metadata]),
            configuration_digest: crate::task_runtime::configuration_digest(),
            invocation_digest: crate::task_runtime::invocation_digest(&session),
            expected_version: hf_backends::backend_launch::CLAUDE_VERSION.into(),
        };
        let payload = encode_invitation(&invitation).unwrap();
        let preparation = PreparedRequest {
            operation: binding.operation.clone(),
            session: invitation.ownership_session.clone(),
            reservation: invitation.reservation.clone(),
            attempt: binding.attempt.clone(),
            generation: binding.generation.clone(),
            digest: hash.digest_parts(&[&payload]),
            payload,
        };
        let intent = LaunchIntentCreate {
            task: binding.task.clone(),
            metadata_digest: invitation.metadata_digest.clone(),
            metadata,
            prompt_digest: invitation.prompt_digest.clone(),
            prompt,
        };
        Seed {
            probe: Probe { root, binding },
            invitation,
            preparation,
            intent,
            env,
            owner: self.chain.parent.clone(),
        }
    }
}
