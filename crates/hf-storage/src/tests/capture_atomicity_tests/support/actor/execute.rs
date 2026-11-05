use super::super::super::super::{Limits, Store};
use super::super::{accepted_binding, deadline};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, FileIdentity},
    ownership::Id,
    process::ProcessExit,
};
use std::io::Write;
use std::path::PathBuf;

pub(in crate::tests::capture_atomicity_tests) fn actor_child() {
    let Ok(root) = std::env::var("HARFLOW_B2C_ROOT") else {
        return;
    };
    let action = std::env::var("HARFLOW_B2C_ACTION").unwrap();
    let hook = std::env::var("HARFLOW_B2C_HOOK").unwrap();
    let root = PathBuf::from(root);
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let binding = accepted_binding(&store, 41);
    store.capture_begin(&binding, deadline()).unwrap();
    if action == "finish" {
        store
            .capture_publish(
                &binding,
                0,
                &CaptureProgress {
                    revision: 1,
                    stdout: 0,
                    stderr: 0,
                    stdout_eof: true,
                    stderr_eof: true,
                    loss: false,
                },
                deadline(),
            )
            .unwrap();
    }
    drop(store);
    println!("ACTOR_READY");
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    assert_eq!(input.trim(), "GO");
    let store = Store::open(&root, Limits::default(), deadline()).unwrap();
    let operation = Id::new(format!("{:032x}", 41)).unwrap();
    let observed = store
        .ownership_observe(&operation, deadline())
        .unwrap()
        .unwrap();
    let binding = CaptureBinding {
        fence: observed.fence().unwrap(),
        child: observed.child.unwrap(),
        stdout: FileIdentity {
            device: 1,
            inode: 1001,
        },
        stderr: FileIdentity {
            device: 1,
            inode: 1002,
        },
    };
    crate::tests::select_stage(hook);
    if action == "progress" {
        store
            .capture_publish(
                &binding,
                0,
                &CaptureProgress {
                    revision: 1,
                    stdout: 3,
                    stderr: 2,
                    stdout_eof: false,
                    stderr_eof: false,
                    loss: false,
                },
                deadline(),
            )
            .unwrap();
    } else {
        store
            .capture_finish(&binding, 1, ProcessExit::Code(0), deadline())
            .unwrap();
    }
}
