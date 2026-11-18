# E2-C S1: caller kernel-record reader

Date: 2026-11-18. Revision-3 base `7addf2a`; option A base `64d018a`. Status: option A implemented, local checks green;
Terminal-chain admission and the release manual smoke pending.

## Option A (current): every ancestry link

Approval: hf-sys `read_into` was first approved for PID 1 only; the owner widened it on 2026-11-18 (coordinator
session, option A) to every ancestry link. Reason, measured under revision 3: libproc `pidinfo` refuses root-owned
non-init processes (EPERM), so Terminal (`login`) and SSH (`sshd`) callers still refused.

- `caller/init_reader.rs` is renamed `caller/kernel_reader.rs`; `LibprocSource` is renamed `SystemSource` and no longer
  uses libproc. `read_record(pid)` rejects pid 0 and values above `i32::MAX`, qualifies on every call (no cache),
  reads `[CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]` into a 1024-byte local buffer, requires exactly 648 bytes and
  decodes with the pid-checked decoder. No fallback and no retry. A nonexistent pid reads zero bytes and refuses.
- `writer_link`, `writer`, `owner_entry` and `recovery/identity.rs` keep libproc for their own reads and compare
  against chain identities, so the invariant is: for the same incarnation under the same boot, the kernel record and
  `MacIdentity::identity` agree on pid, start seconds and microseconds, and the record agrees with libproc `pbi_ppid`
  and `pbi_uid`. The tests sample this; they do not prove it, and root-owned processes cannot be cross-checked.
- Tests: eight in-crate tests in `kernel_reader/tests.rs` (fixture decode for init and non-init with pid forwarding
  checked inside it, mismatched-pid and invalid-pid refusal before any read, lengths, errno, unqualified host with zero
  reads, unsupported architecture, boot bookend change) and one in `kernel_reader/os_tests.rs`
  (`kernel_reader_reads_root_owned_non_init_process`). OS tests in `tests/caller_system_reader.rs`: end-to-end,
  live ancestry, identity equality with libproc for the effective-uid links of the test ancestry and two spawned
  `/bin/sleep` children, expired deadline, and `observe_caller_admits_root_owned_ancestor_when_present`. The
  same-uid search runs under one absolute 20 s deadline covering `ps`, the preflight walks and `observe_caller`; the
  50-attempt cap counts only candidates whose ancestry has a root-owned non-init link, while all same-uid rows are
  scanned within the budget. A refusal is retried only when a re-walk shows a link gone or changed; an unchanged
  walk or a read error fails, and an expired budget is reported as "search budget exhausted". The revision-3
  libproc-refusal branch is removed.
- D10 results (agent shell, uid 503): root-owned evidence branch ran. The kernel reader decoded 20 root daemons
  (logd, sandboxd, smd, UserEventAgent, fseventsd, mediaremoted, systemstats, configd, endpointsecurityd, powerd,
  IOMFB_bics_daemon, amfid, remoted, keybagd, watchdogd, mds, kernelmanagerd, diskarbitrationd, coreduetd, syslogd), each
  stable across two reads, all with parent 1, and libproc refused every one. The admission test found no root-owned
  non-init link in the test process's own ancestry and no same-uid process under one ("no root-ancestor chain
  available"), so the live Terminal/SSH-chain admission was not exercised here. Supporting evidence only: root-daemon
  reads and the uniform algorithm. Owner action: run `cargo test -p hf-platform --test caller_system_reader --
  --nocapture` once from Terminal.app and record the printed `D10b branch`.
- Revision-3 deviation (a) (hybrid, PID 1 only) is withdrawn; deviation (b) (qualification key) stands.
- The pin now names `kernel_reader.rs`. Everything below this section is revision-3 history (PID 1 only), kept as
  evidence; where it says non-init links stay on libproc it is superseded.

## Revision 3 history: PID 1 only

## Change

The init anchor of the caller observation is read with hf-sys `read_into` on
`[CTL_KERN, KERN_PROC, KERN_PROC_PID, 1]` into a local 1024-byte buffer. The returned length must be exactly
648 bytes, and the record is decoded with `decode_caller_record(MacOsArm64Sdk26_5, 1, ..)`, which validates PID 1,
parent 0, status and start time. Every non-init link keeps the libproc `BSDInfo` read, so process identities compared
by the writer, owner and recovery code are produced as before. There is no libproc fallback for PID 1 and no retry.

Deviations from the parent S1 text: only PID 1 uses the new reader (the parent text used it for every PID), and the
qualification key is `(kern.osrelease, kern.osversion)` rather than `kern.osproductversion`
(which returns EPERM under the restrictive sandbox profile).

## Qualification

The layout is selected only when `std::env::consts::ARCH == "aarch64"` and the strings read with the existing
`sysctl` crate are in the allowlist `[("25.6.0", "25G83")]`. The architecture gate exists twice: the whole
`task_runtime` module is compiled only on macOS arm64, and the check runs again at run time. Qualification runs on
every init read (twice per observation); there is no cached state. A read error or unlisted value refuses without
reading the record. Trade-off: any OS update, security response or build change (for example 25G84) makes
every caller observation refuse until that build is qualified and added by a reviewed change. Measured `kern.osproductversion`
is 26.6.2, recorded for reference only.

## Boundary pin

`scripts/check-architecture.py` accepts the token `hf_sys` in hf-platform only in
`src/task_runtime/caller/kernel_reader.rs` (then `init_reader.rs`) and `tests/caller_system_reader.rs`. Text is scanned with comments removed
and strings masked. Rules:

- The allowed files may not contain `pub use`, `pub extern crate` or `macro_rules!`.
- `include!` is rejected in hf-platform.
- Any `path =` inside an attribute, including `cfg_attr`, with normal or raw strings, is rejected when the value
  contains a backslash escape, is absolute, has a `..` component after `posixpath.normpath`, matches an allowed file
  by casefolded path suffix after normalisation (`./`, `//` and case variants), or resolves from the file's directory
  to a file that `os.path.samefile` matches to an allowed file.
- Existing sibling `#[path]` test and support modules and `include_str!` remain accepted, so the plan's blanket ban
  was narrowed.

The screen is textual, not a proof over all Rust references.

## Tests

Six in-crate unit tests with an injected platform (fixture decode, bad lengths 0/647/649/1024, errno
ENOMEM/EPERM/ESRCH, unqualified strings and string-read error with zero reads, pure unsupported architecture,
boot-UUID bookend change through `observe_caller_with_source`). Four OS-enabled tests in
`tests/caller_system_reader.rs` (end-to-end `current_caller`, live ancestry to init, kernel record equals libproc for
self and parent in pid, parent, uid and start time, expired deadline refuses); each first evaluates qualification and
asserts the explicit refusal on an unqualified host. The OS tests branch at start: on an unqualified host, or when a root-owned non-init ancestor is refused by libproc
(a Terminal or SSH session under `login`/`sshd`), only the explicit-refusal branch runs. In the three runs from the
agent shell (all ancestors uid 503) the positive branch ran and all four tests passed. Policy tests were added to
`tests/test_architecture_unsafe_boundary.py`. The existing caller decoder and ancestry tests are unchanged.

## Measurements (agent shell, macOS 26.6.2 arm64)

- `kern.osrelease` 25.6.0 and `kern.osversion` 25G83 are readable (sysctl crate, unprivileged). Not measured under
  the restrictive sandbox profile.
- Ancestry of the test process: zsh (uid 503) -> claude (503) -> disclaimer (503) -> Claude.app (503) -> init. libproc
  `pidinfo` succeeded for all of them. No root-owned non-init ancestor existed in this tree, so `login`/`sshd` were not
  observed as ancestors.
- libproc `pidinfo` on root-owned non-init processes fails with EPERM (Operation not permitted): logd (385), sandboxd
  (452), backupd (567), applekeystored (672), diagnosticd (3705). The same PIDs read through `kern.proc.pid`
  succeed with 648 bytes each.

Consequence for the hybrid: a caller whose ancestry contains a root-owned non-init process (for example `login` or
`sshd` for a Terminal or SSH session) will still be refused by the libproc reads of that link, even though the
init anchor now succeeds. This is not resolved by S1; it needs a decision on reading non-init links through the same
qualified record, which changes the identities compared elsewhere.

## Manual smoke: pending

The release `harflow-ctl run` smoke with a scratch `HOME` and `HARFLOW_STATE_ROOT`, and its positive oracle, has not
been run. The coordinator or owner records the observed ancestry (including root-owned links), output, binary hash
and source commit here.
