# E2-C S1: PID 1 caller evidence reader

Date: 2026-11-18. Base: `7addf2a`. Status: implemented, local checks green; release manual smoke pending.

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
`src/task_runtime/caller/init_reader.rs` and `tests/caller_system_reader.rs`. Text is scanned with comments removed
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
