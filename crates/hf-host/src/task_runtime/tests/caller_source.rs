//! Scripted caller evidence.
//!
//! The chain is the test process T, its real parent P, then synthetic ancestors PP and
//! PPP and a synthetic init. T and P carry their real identities, so the identity of P
//! still equals what `current_parent_identity` reports. A rule changes or fails one
//! observation, counted per starting pid.
use crate::task_runtime::seams::CallerHook;
use hf_application::process::IdentityPort as _;
use hf_domain::process::ProcessIdentity;
use hf_platform::{
    process::{MacIdentity, current_parent_identity},
    task_runtime::caller::{
        CallerEvidenceSource, CallerObservation, CallerProcessRecord, observe_caller_with_source,
    },
};
use std::{
    collections::HashMap,
    io,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

const PP: u32 = 71_001;
const PPP: u32 = 71_002;

#[derive(Clone, Debug)]
struct Node {
    pid: u32,
    parent: u32,
    seconds: u64,
    micros: u64,
}
#[derive(Clone, Debug)]
struct Topology {
    nodes: Vec<Node>,
    boot: String,
    uid: u32,
    flip: bool,
    failing: Vec<(u32, i32)>,
}
/// What one scripted observation does differently.
#[derive(Clone, Debug)]
pub(super) enum Effect {
    /// The record of `pid` cannot be read (for example EPERM or ESRCH).
    Fail(u32, i32),
    /// `pid` reports another parent.
    Reparent(u32, u32),
    /// `pid` reports another start time, as a recycled pid would.
    Restart(u32),
    /// The boot session identifier changes within one record read.
    BootFlip,
}
struct Rule {
    pid: u32,
    nth: usize,
    effect: Effect,
}
struct State {
    topology: Topology,
    rules: Vec<Rule>,
    counts: HashMap<u32, usize>,
}
pub(super) struct Scripted {
    state: Mutex<State>,
}
/// The identities of the chain.
pub(super) struct Chain {
    pub(super) test: ProcessIdentity,
    pub(super) parent: ProcessIdentity,
    pub(super) dispatcher: ProcessIdentity,
    pub(super) caller: ProcessIdentity,
}
impl Scripted {
    pub(super) fn new(uid: u32) -> Arc<Self> {
        let test = MacIdentity
            .identity(std::process::id())
            .expect("a qualified test process");
        let parent = current_parent_identity().expect("a qualified parent process");
        let node = |pid, parent, seconds, micros| Node {
            pid,
            parent,
            seconds,
            micros,
        };
        let nodes = vec![
            node(
                test.pid(),
                parent.pid(),
                test.start_seconds(),
                test.start_microseconds(),
            ),
            node(
                parent.pid(),
                PP,
                parent.start_seconds(),
                parent.start_microseconds(),
            ),
            node(PP, PPP, 1_700_000_100, 1),
            node(PPP, 1, 1_700_000_050, 2),
            node(1, 0, 1_700_000_000, 3),
        ];
        Arc::new(Self {
            state: Mutex::new(State {
                topology: Topology {
                    nodes,
                    boot: test.boot_uuid().to_owned(),
                    uid,
                    flip: false,
                    failing: Vec::new(),
                },
                rules: Vec::new(),
                counts: HashMap::new(),
            }),
        })
    }
    pub(super) fn chain(&self) -> Chain {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let identity = |pid: u32| {
            let node = state.topology.nodes.iter().find(|n| n.pid == pid).unwrap();
            ProcessIdentity::new(pid, node.seconds, node.micros, state.topology.boot.clone())
                .unwrap()
        };
        let test_pid = std::process::id();
        let parent_pid = state.topology.nodes[1].pid;
        Chain {
            test: identity(test_pid),
            parent: identity(parent_pid),
            dispatcher: identity(PP),
            caller: identity(PPP),
        }
    }
    /// Applies `effect` to the `nth` (1-based) observation that starts at `pid`.
    pub(super) fn rule(&self, pid: u32, nth: usize, effect: Effect) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.rules.push(Rule { pid, nth, effect });
    }
    pub(super) fn pids() -> (u32, u32) {
        (PP, PPP)
    }
    pub(super) fn observe(&self, pid: u32, deadline: Instant) -> io::Result<CallerObservation> {
        let topology = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let count = state.counts.entry(pid).or_insert(0);
            *count += 1;
            let nth = *count;
            let mut topology = state.topology.clone();
            for rule in state.rules.iter().filter(|r| r.pid == pid && r.nth == nth) {
                apply(&mut topology, &rule.effect);
            }
            topology
        };
        observe_caller_with_source(
            pid,
            deadline,
            &mut Records {
                topology,
                boot_calls: 0,
            },
        )
    }
    pub(super) fn hook(self: &Arc<Self>) -> Rc<CallerHook> {
        let source = self.clone();
        Rc::new(move |pid, deadline| source.observe(pid, deadline))
    }
}
fn apply(topology: &mut Topology, effect: &Effect) {
    match effect {
        Effect::Fail(pid, errno) => topology.failing.push((*pid, *errno)),
        Effect::Reparent(pid, parent) => {
            if let Some(node) = topology.nodes.iter_mut().find(|n| n.pid == *pid) {
                node.parent = *parent;
            }
        }
        Effect::Restart(pid) => {
            if let Some(node) = topology.nodes.iter_mut().find(|n| n.pid == *pid) {
                node.seconds += 1;
            }
        }
        Effect::BootFlip => topology.flip = true,
    }
}
struct Records {
    topology: Topology,
    boot_calls: usize,
}
impl CallerEvidenceSource for Records {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn boot_uuid(&mut self) -> io::Result<String> {
        self.boot_calls += 1;
        if self.topology.flip && self.boot_calls.is_multiple_of(2) {
            return Ok("00000000-0000-4000-8000-000000000001".into());
        }
        Ok(self.topology.boot.clone())
    }
    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord> {
        if let Some((_, errno)) = self.topology.failing.iter().find(|(p, _)| *p == pid) {
            return Err(io::Error::from_raw_os_error(*errno));
        }
        let node = self
            .topology
            .nodes
            .iter()
            .find(|n| n.pid == pid)
            .ok_or_else(|| io::Error::from_raw_os_error(3))?;
        Ok(CallerProcessRecord {
            pid,
            parent: node.parent,
            uid: if pid == 1 { 0 } else { self.topology.uid },
            status: 2,
            start_seconds: node.seconds,
            start_microseconds: node.micros,
        })
    }
    fn real_uid(&mut self) -> u32 {
        self.topology.uid
    }
}
