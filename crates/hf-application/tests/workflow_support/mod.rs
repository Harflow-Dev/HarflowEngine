//! Test doubles for the workflow ports. `backend_facts.rs` is generated from the corpus by
//! `gen_r10a_corpus.py`; the rest is hand-written.
#![allow(dead_code, unused_imports)]

pub mod backend_facts;
mod catalog;
mod codec;
mod misc;
mod store;

pub use catalog::TableCatalog;
pub use codec::FakeCodec;
pub use misc::{FixedClock, MapResolver, SeqIds};
pub use store::{MemStore, StoreCall};
