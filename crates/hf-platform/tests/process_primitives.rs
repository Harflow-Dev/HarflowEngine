#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "process_primitives/common.rs"]
mod common;
#[path = "process_primitives/identity.rs"]
mod identity;
#[path = "process_primitives/lifecycle.rs"]
mod lifecycle;
#[path = "process_primitives/races.rs"]
mod races;
