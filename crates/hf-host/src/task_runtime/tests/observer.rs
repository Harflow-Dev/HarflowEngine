//! The phased observer of an owner in permanent retention.
//!
//! Phases: Gate (run the gate action once), Observe (wait for the predicate and take a
//! snapshot), Cleanup, Unwind. All phases share one budget; when it runs out the failure is
//! recorded and the observer goes to Cleanup. Cleanup runs while the observer still holds
//! the runtime, and only then does it unwind with `Retained`.
use super::harness::{Cleanup, Retained};
use crate::task_runtime::{owner_runtime::Runtime, seams::OwnerRetainedHook};
use std::{
    cell::{Cell, RefCell},
    panic::resume_unwind,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Gate,
    Observe,
    Cleanup,
}
/// What the observer found, read after the unwind.
pub(super) struct Findings<S> {
    snapshot: Rc<RefCell<Option<S>>>,
    failure: Rc<RefCell<Option<String>>>,
}
impl<S> Findings<S> {
    /// The snapshot, or the failure that ended the observation.
    pub(super) fn take(&self) -> S {
        if let Some(failure) = self.failure.borrow().as_ref() {
            panic!("observer failed: {failure}");
        }
        self.snapshot
            .borrow_mut()
            .take()
            .expect("no snapshot taken")
    }
}
/// Builds the retention hook: `gate` once, then `until` on every retention step until it
/// returns a snapshot.
pub(super) fn observe<S: 'static>(
    cleanup: Rc<Cleanup>,
    budget: Duration,
    gate: impl Fn(&mut Runtime) + 'static,
    until: impl Fn(&mut Runtime) -> Option<S> + 'static,
) -> (Rc<OwnerRetainedHook>, Findings<S>) {
    let findings = Findings {
        snapshot: Rc::new(RefCell::new(None)),
        failure: Rc::new(RefCell::new(None)),
    };
    let (snapshot, failure) = (findings.snapshot.clone(), findings.failure.clone());
    let phase = Cell::new(Phase::Gate);
    let end = Instant::now() + budget;
    let hook = Rc::new(move |runtime: &mut Runtime| match phase.get() {
        Phase::Gate => {
            gate(runtime);
            phase.set(Phase::Observe);
        }
        Phase::Observe => {
            if let Some(found) = until(runtime) {
                *snapshot.borrow_mut() = Some(found);
                phase.set(Phase::Cleanup);
            } else if Instant::now() >= end {
                *failure.borrow_mut() = Some("predicate not reached within the budget".into());
                phase.set(Phase::Cleanup);
            }
        }
        Phase::Cleanup => {
            cleanup.owner(runtime);
            resume_unwind(Box::new(Retained));
        }
    });
    (hook, findings)
}
