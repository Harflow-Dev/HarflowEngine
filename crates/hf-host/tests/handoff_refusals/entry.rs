#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use super::process::{ChildGuard, Seed, deadline};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub(crate) enum EntryFault {
    Environment,
    ExtraDescriptor,
    RegularStdin,
}

pub(crate) fn entry_fault(seed: &Seed, fault: EntryFault) -> ChildGuard {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold"));
    let mode = OsString::from("--internal-owner-fixture-v1");
    match fault {
        EntryFault::Environment => ChildGuard::spawn_with_env(
            binary,
            vec![mode],
            seed.scratch.0.clone(),
            vec![(OsString::from("HF_B2_ENTRY_PROBE"), OsString::from("1"))],
            deadline(8),
        ),
        EntryFault::ExtraDescriptor | EntryFault::RegularStdin => {
            let path = seed.scratch.0.join("entry-probe");
            fs::write(&path, b"inert descriptor probe").expect("create probe file");
            python_exec_owner(seed, &path, matches!(fault, EntryFault::RegularStdin))
        }
    }
}

fn python_exec_owner(seed: &Seed, path: &Path, regular_stdin: bool) -> ChildGuard {
    let script = if regular_stdin {
        "import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.dup2(fd,0); os.close(fd); os.execve(sys.argv[2],[sys.argv[2],sys.argv[3]],{})"
    } else {
        "import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.set_inheritable(fd,True); os.execve(sys.argv[2],[sys.argv[2],sys.argv[3]],{})"
    };
    ChildGuard::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            OsString::from("-I"),
            OsString::from("-S"),
            OsString::from("-c"),
            OsString::from(script),
            path.as_os_str().to_owned(),
            PathBuf::from(env!("CARGO_BIN_EXE_harflow-engine-scaffold")).into_os_string(),
            OsString::from("--internal-owner-fixture-v1"),
        ],
        seed.scratch.0.clone(),
        deadline(8),
    )
}

pub(crate) fn finish_entry_refusal(child: &mut ChildGuard) {
    child.0.close_stdin();
    child
        .0
        .fixture_eof(deadline(5))
        .expect("entry refusal closes output before protocol");
    child.finish(deadline(5), 2);
}
