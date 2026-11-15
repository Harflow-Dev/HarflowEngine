//! A store survey: every check `qualify::precheck` makes, each kept as its own outcome.
//!
//! A check runs only when its prerequisites passed; otherwise it is recorded
//! `NotPerformed` and never executed, so no file is opened after a refusal that makes
//! opening it unsafe.
//!
//! * `Mode::Precheck` is strictly linear: every check is a prerequisite of the next, so a
//!   survey performs exactly the I/O of the sequence it replaces, in the same order, and
//!   `require_clean` returns the first failure. It is usable under shared leases.
//! * `Mode::Maintenance` is constructible only with the `Authority` a `MaintenanceSession`
//!   owns. Sidecar safety, marker and header are prerequisites of everything; the later
//!   checks do not depend on each other, so a missing `-wal` is reported together with the
//!   fence and watermark. Observation confers no authority: nothing outside the session
//!   can obtain an unclean survey.
use crate::{
    Error, Identity, Result, binding::Binding, connection, fence, maintenance::Authority, wal,
    wal_acceptance, watermark,
};
use std::{fs, path::Path};

/// One check, in the order `precheck` makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    Sidecars,
    Marker,
    Header,
    Journal,
    Wal,
    Acceptance,
    Fence,
    Watermark,
    Binding,
}
const ORDER: [Check; 9] = [
    Check::Sidecars,
    Check::Marker,
    Check::Header,
    Check::Journal,
    Check::Wal,
    Check::Acceptance,
    Check::Fence,
    Check::Watermark,
    Check::Binding,
];
/// Checks before this index are prerequisites of every later check in maintenance mode.
const BASE: usize = 3;

impl Check {
    fn index(self) -> usize {
        ORDER.iter().position(|check| *check == self).unwrap_or(0)
    }
}

/// What one check found.
#[derive(Debug)]
pub(crate) enum Outcome {
    Passed,
    Failed(Error),
    /// A prerequisite did not pass, so the check never ran.
    NotPerformed,
}

#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    Precheck,
    /// The authority is a proof carried by value of the type, not read.
    Maintenance(#[allow(dead_code)] &'a Authority),
}

/// The individual steps, so a test can count or script them.
pub(crate) trait Steps {
    fn sidecars(&self, binding: &Binding) -> Result<()>;
    fn marker(&self, binding: &Binding) -> Result<Identity>;
    fn header(&self, binding: &Binding) -> Result<()>;
    fn journal(&self, binding: &Binding) -> Result<()>;
    fn wal(&self, binding: &Binding) -> Result<()>;
    fn acceptance(&self, root: &Path, generation: &str) -> Result<()>;
    fn fence(&self, root: &Path, generation: &str) -> Result<()>;
    fn watermark(&self, root: &Path, generation: &str) -> Result<u64>;
    fn binding(&self, binding: &Binding) -> Result<()>;
}

/// The real checks.
pub(crate) struct Live;
impl Steps for Live {
    fn sidecars(&self, binding: &Binding) -> Result<()> {
        binding.check_sidecars(false)
    }
    fn marker(&self, binding: &Binding) -> Result<Identity> {
        wal::marker(binding)
    }
    fn header(&self, binding: &Binding) -> Result<()> {
        connection::qualify_header(&binding.database())
    }
    fn journal(&self, binding: &Binding) -> Result<()> {
        wal::reject_nonempty_journal(binding)
    }
    fn wal(&self, binding: &Binding) -> Result<()> {
        require_wal(binding)
    }
    fn acceptance(&self, root: &Path, generation: &str) -> Result<()> {
        wal_acceptance::check(root, generation)
    }
    fn fence(&self, root: &Path, generation: &str) -> Result<()> {
        fence::check(root, generation)
    }
    fn watermark(&self, root: &Path, generation: &str) -> Result<u64> {
        watermark::read(root, generation)
    }
    fn binding(&self, binding: &Binding) -> Result<()> {
        binding.check()
    }
}

/// `lstat` only: a missing or non-regular `-wal` is not a legitimate engine state.
fn require_wal(binding: &Binding) -> Result<()> {
    match fs::symlink_metadata(binding.root().join("store.sqlite3-wal")) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(Error::WalMissing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::WalMissing),
        Err(error) => Err(error.into()),
    }
}

pub(crate) struct Survey {
    outcomes: Vec<Outcome>,
    identity: Option<Identity>,
    mark: Option<u64>,
}

impl Survey {
    pub(crate) fn outcome(&self, check: Check) -> &Outcome {
        &self.outcomes[check.index()]
    }
    /// The marker's identity, when the marker check passed.
    pub(crate) fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }
    /// The watermark sequence, when the watermark check passed.
    pub(crate) fn mark(&self) -> Option<u64> {
        self.mark
    }
    /// The first failure in `precheck` order, or the marker identity and watermark
    /// sequence when every check passed.
    pub(crate) fn require_clean(self) -> Result<(Identity, u64)> {
        for outcome in self.outcomes {
            match outcome {
                Outcome::Passed => {}
                Outcome::Failed(error) => return Err(error),
                Outcome::NotPerformed => return Err(Error::Incompatible),
            }
        }
        match (self.identity, self.mark) {
            (Some(identity), Some(mark)) => Ok((identity, mark)),
            _ => Err(Error::Incompatible),
        }
    }
}

/// The survey of the checks `precheck` makes, in its order, stopping at the first failure.
pub(crate) fn precheck(binding: &Binding) -> Result<(Identity, u64)> {
    run(binding, Mode::Precheck, &Live).require_clean()
}

/// The maintenance survey; the session's `Authority` is the proof it holds the locks.
pub(crate) fn maintenance(binding: &Binding, authority: &Authority) -> Survey {
    run(binding, Mode::Maintenance(authority), &Live)
}

pub(crate) fn run(binding: &Binding, mode: Mode<'_>, steps: &dyn Steps) -> Survey {
    let mut survey = Survey {
        outcomes: ORDER.iter().map(|_| Outcome::NotPerformed).collect(),
        identity: None,
        mark: None,
    };
    let root = binding.root();
    for check in ORDER {
        if !ready(&survey, mode, check) {
            continue;
        }
        let generation = survey.identity.as_ref().map(|i| i.generation.clone());
        let generation = generation.as_deref().unwrap_or_default();
        let result = match check {
            Check::Sidecars => steps.sidecars(binding),
            Check::Marker => steps.marker(binding).map(|identity| {
                survey.identity = Some(identity);
            }),
            Check::Header => steps.header(binding),
            Check::Journal => steps.journal(binding),
            Check::Wal => steps.wal(binding),
            Check::Acceptance => steps.acceptance(root, generation),
            Check::Fence => steps.fence(root, generation),
            Check::Watermark => steps.watermark(root, generation).map(|mark| {
                survey.mark = Some(mark);
            }),
            Check::Binding => steps.binding(binding),
        };
        survey.outcomes[check.index()] = match result {
            Ok(()) => Outcome::Passed,
            Err(error) => Outcome::Failed(error),
        };
    }
    survey
}

/// Whether every prerequisite of `check` passed.
fn ready(survey: &Survey, mode: Mode<'_>, check: Check) -> bool {
    let needed = match mode {
        Mode::Precheck => check.index(),
        Mode::Maintenance(_) => check.index().min(BASE),
    };
    survey.outcomes[..needed]
        .iter()
        .all(|outcome| matches!(outcome, Outcome::Passed))
}
