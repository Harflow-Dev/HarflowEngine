//! Private single-thread fixture adapters. No general runtime launch policy.
mod channel;
mod gate;
mod ledger;
mod policy;
pub use channel::{PrivateIo, read_frame, require_eof, write_all};
pub use gate::{FixtureGate, gate_packet};
pub use policy::check_entry;

pub use ledger::{LedgerIdentity, append_effect, qualify_ledger};
