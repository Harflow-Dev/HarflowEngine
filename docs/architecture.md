# Architecture

hf-domain owns pure values; hf-application owns ports and use cases and depends only on domain. hf-storage provides the internal fixture foundation described in [storage foundation](storage-foundation.md); hf-backends contains diagnostic fixture construction without a production runtime and hf-platform owns bounded process/channel adapters. hf-interfaces maps transports. hf-host composes adapters. hf-contracts is deliberately empty: canonical schemas remain in HarflowMCP. hf-architecture is build-time tooling only. Backend execution and client-registration adapters remain conceptually distinct. The binaries are harflow-engine-scaffold and harflow-ctl (hf-host) and the private tools hf-architecture and hf-fixture-seed. The scaffold's default invocation prints an unsupported diagnostic and reads/writes no engine state. harflow-ctl exposes the public observer/read surface: `version-info` and `state-inspect` (a lease-free observer that opens no SQLite database and never creates anything) everywhere, and on macOS arm64 the store-backed reads `list`, `status` (with `--brief`), `status-batch`, `task-list-page` (including successor discovery), `task-events`, `task-prompt`, `task-artifacts`, `recovery-activity`, `interaction-events`, `statistics-page`, `task-pending-page` and `task-pending-status`. The reads classify the root with the same observer first, open the store only through the read session under a shared lease, run each command inside one snapshot and create nothing, including for a missing root. Authority-gated reads apply the caller gate of `hf_domain::ctl::caller`, whose two named switches (`REFUSE_UNRESOLVED_AGENTS`, `MISSING_ROOT_READS_EMPTY`) are the only places the owner decisions on unresolved agents and missing roots are encoded; the admitted class is `NoAgentDetected`, never a human credential. harflow-ctl advertises `ctl.v1` and `version-info.v1`, plus `task-pending-launch.v1`, `task-successors.v1`, `task-event-page.v1`, `task-prompt.v1`, `task-artifacts.v1`, `task-continuation-stop.v1`, `interaction-events.v1` and `statistics-page.v1` on macOS arm64, and `state-recovery.v1` immediately after `version-info.v1` on macOS arm64. The public `state-recover --operation-id OP --confirmation-token TOKEN [--json]` path on that platform composes explicit confirmed recovery under the original MaintenanceSession; unsupported platforms refuse it as `invalid choice`, exit 2. Other unimplemented paths also answer `invalid choice`, exit 2. Its code lives in a `ctl` module per layer and may not name the private fixture modes below. R14's target is macOS arm64; Linux is W02 and Windows WIN02, so CI there covers platform-independent code only.  Explicit private fixture modes described below operate only on caller-supplied isolated Rust fixture roots.

Target files below 250 substantive lines and functions below 50 code lines. Enforced production maxima are 400 and 80. Changes must preserve constructor injection, explicit errors and bounded work. No compatibility or migration claim follows from a successful build.

hf-sys is the workspace's only `unsafe` boundary. It denies `unsafe_code` instead of forbidding it and carries exactly one `#[expect(unsafe_code)]`, on the `sysctl` call in `read_into`; its only dependency is libc, and only hf-platform depends on it, on macOS only. It exists because the workspace forbids `unsafe` and a caller-buffer `kern.proc.pid` read is needed where libproc refuses PID 1 and root-owned non-init processes. Its one production caller is the caller kernel-record reader (`task_runtime/caller/kernel_reader.rs`), pinned by the architecture guard to that file and its OS-enabled test. On other operating systems the crate is empty, and its sources are still scanned. rustc enforces the boundary: `forbid` in every other crate and `deny` in hf-sys, so a second unsafe block compiles only with another lint override. The guard pins that override, rejects macro definitions in hf-sys and in-repo Cargo or rustc configuration that could lower lint levels (`.cargo/config`, root `cargo-features`, profile `rustflags`, a non-stable or missing `rust-toolchain.toml` pin). Environment `RUSTFLAGS` and the caller's own Cargo home config stay outside the guard. For the pin, raw text under hf-sys/src must hold exactly one `unsafe_code` token, as an outer `#[expect]` directly on one `let ... = unsafe {` statement, and no `#[path]` or `include!`. What that block contains is a human-review responsibility. Outside hf-sys the guard also runs a conservative text screen (comments and string and character literals removed) for `unsafe` forms and `unsafe_code` relaxations; it is defence in depth, not a proof.

The metadata guard enforces the exact member inventory, approved normal dependency edges (including tooling syn), no dev/build dependencies, inherited workspace lints (hf-sys excepted, as above) and all crates/tools Rust file sizes. Clippy enforces 80 code lines per function.

The domain guard parses every .rs file under hf-domain with syn. It rejects standard-library roots except the documented value-oriented modules option/result/cmp/clone/fmt/marker/string/vec/boxed/collections/iter/convert/default/mem/primitive. Direct and aliased fs/process/env/time/thread/OS imports are rejected before alias use. Root aliases, glob imports, extern crate, unsafe expressions, macros and attributes other than literal string documentation/built-in derives are rejected. This is conservative syntax enforcement, not semantic effect inference; adding external crates or macro expansion requires explicit policy revision. It does not claim whole-program purity for arbitrary future code.

Future execution must specify detached ownership, crash recovery, mutation uncertainty, bounded work and capability negotiation before implementation. Platform effects stay in hf-platform. Never introduce a second writer for Python-owned engine state.

The domain metadata check rejects build scripts and custom target source roots; only the conventional src/lib.rs library target is accepted. Documentation macros are rejected even inside attributes.

Inline modules gated by the exact #[cfg(test)] attribute are exempt from source purity analysis, allowing ordinary unit-test attributes and assertion macros. Out-of-line test modules and files named tests.rs receive no exemption. Production cfg modules and cfg_attr remain subject to the policy. Keep test-specific Clippy allowances inside the inline test module; production denies remain inherited. Non-domain crate roots allow unwrap/expect/panic only under cfg(test).

hf-storage also provides a read session that answers task and event reads from the real store read-only under a shared lease; it is specified in the [read-path design](store-read-path.md) and the [storage protocol](storage-protocol.md); the ctl reads listed above are its only production callers. Internal task-read values, ports, fixture queries and consumer-only mappings are
described in [task reads](task-reads.md). They are the internal seam beneath the ctl read
commands and add no host command of their own or production lifecycle verification.

Internal stored event paging and consumer-only mappings are described in
[event reads](event-reads.md). They retain fixture authority and observation
boundaries; that stage added no public host operation or watcher, and no watch command
or `task-list-watch.v1` capability exists until E2.

The macOS arm64 fixture-only [process primitives](process-primitives.md) retain
qualified direct-child handles and independent capture/exit evidence. They add no
detached owner, public execution capability or descendant-containment guarantee.

The [durable ownership foundation](ownership-foundation.md) adds isolated internal
intent/reservation/claim transactions and fenced observations. The original B1 stage selected
schema2; current fresh roots use schema 8 as described in the fixture seed paragraph below. No automatic upgrade,
detached runtime or public execution API is added.

The [private fixture owner handoff](owner-handoff.md) composes B1 with actual owned
fixture processes through bounded inherited channels. Fresh single-thread parent,
owner and fixed effect modes reject ambient environment/unexpected descriptors,
historically closed every Store handle before spawn and preserve reservations after uncertainty.
They do not qualify a general concurrent launcher, detached runtime, backend,
public lifecycle operation or durable capture/terminal ordering. No schema upgrade
or public capability is introduced by these private modes.

The [task-list invalidation foundation](task-list-watch.md) introduced
fresh schema3 roots and joins admitted task metadata writes to the same atomic
revision commit. It adds bounded waits and strict corrected0.0.10 consumer mappings;
the full207-case bundle is now selected with exhaustive provenance. No watch CLI,
production caller authority, task catalog lifecycle or public capability is added.

The [private capture/completion foundation](capture-completion.md) extends that
fixed owner with synced raw bytes, fenced private offset/events and terminal
ordering. That stage introduced schema4; earlier schema3/2 descriptions record
their owning stages. It retains reservations and Unknown containment, and adds no
public runtime, restart adoption or R04 qualification.

The [read-only recovery observation foundation](recovery-observation.md) inspects
one explicit fixture operation using bounded private-copy snapshots and independent
identity/raw evidence. That stage preserved schema4 and adds no recovery intent, source
repair, process control, adoption, release or replacement authority.

The internal [R04-C concurrent launch slice](migration-receipts/R04-C.md) introduces
fresh schema5 roots to exclude old locking protocols, explicit application lease
ports, a shared platform record-lock coordinator and safe Darwin descriptor-allowlisted
spawn with sole-wait authority. Schema4 roots refuse without conversion. Store leases
can remain held across launch; historical fixture close-before-spawn rules are no
longer the concurrent isolation mechanism. Requested differing CWD is refused by
the safe seam. This remains internal qualification, not a production owner runtime
or new public capability. R04-B's policy recommendations were not accepted at that stage; see
the [owner-runtime candidate](owner-runtime.md) for the approvals that now govern.

The bounded [R04-D D1 owner foundation](migration-receipts/R04-D.md) retains a
stateful application OwnerSession and its narrow injected Store port across the
fixed helper lifecycle. Exact fenced transitions and permanent poisoning after
uncertain writes do not grant public caller authority. Raw-sentinel prewarming
respects R04-C's per-process descriptor cap. That stage used synchronous capture. The subsequent private
[output-drain design](owner-output-drain.md) moves only stdout/stderr to one bounded
reader while retaining persistence on the original owner main thread. Its finite
FIFO, sticky discard and immutable final observations preserve poison and terminal
ordering. Private cleanup settles reader and original child before root teardown;
indefinitely blocked main-thread storage still has no bounded shutdown guarantee.
Production caller/lifecycle budgets and later policy decisions remain separate gates.

The [owner-runtime candidate](owner-runtime.md) records the approved production
policies and private isolated-storage implementation boundaries. Synthetic model
inputs and fixed-helper mechanics do not qualify public execution or OS enforcement.

[Backend/OS measurements](backend-os-qualification.md) add only fresh-root
measurement adapters and explicit stock-route refusals; their observations never
become public execution authority.

The [offline Claude foundation](claude-offline-foundation.md) separates bounded
JSONL transport decoding, pure backend reduction, declared context correlation and
read-only owner observation. Synthetic completion evidence never admits a
production route or changes the fixed-helper launch contract.

The [prepared fixed-helper launch foundation](prepared-launch-foundation.md)
adds a distinct closed preparation and original-owner byte commitment, composed
with exact current raw-source validation. It preserves one-use handoff, claim CAS
and unknown-outcome boundaries. The linked real-Claude experiment is a proposal;
no backend dispatch or production admission follows from these private mechanics.

The private [live Claude authentication/egress probe](claude-live-auth-egress.md)
adds memory-only credential handling and a destination-locked CONNECT diagnostic.
It is a separate direct-child slice, not live-owner execution or a public backend
route. Its fake-token TLS gate remains unqualified, so its real-credential branch
has not run. The [decision](decisions/claude-live-auth-egress.md) records its bounds
and explicitly unqualified retry, billing and production guarantees.

The separate [two-tunnel fake-only TLS control](claude-two-tunnel-negative.md) is an
owner-approved bounded diagnostic. It adds no real-provider route, credential
access, public capability or production admission; its own receipt determines
whether the stricter negative-control evidence is qualified.

The private [R06-A conversation ledger](migration-receipts/R06-A.md) is an offline,
same-context, Queue-only fixture over injected ports and reserved `__r06/` fixture
records. It adds no delivery transport, public capability or production evidence
adapter. Every other route refuses before port I/O.

Store schema 8 adds fenced `lifecycle_records` binding task, operation, attempt and root generation, with an independent lifecycle CAS revision and durable execution observations. Schema 7 introduced `launch_attempts` and `chain_stops`; schema 7 and older roots refuse as `unsupported_layout`, without automatic conversion. Lifecycle evidence publication does not publish terminal status, EOF, settlement or release; that canonical projection remains a separate gate. Event/source publication requires an explicit `DurableCaptureAdmission` application port. This foundation supplies no production raw-capture adapter: real source identity, sync and known-loss admission remain an owner-composition qualification gate, so this source seam alone does not qualify T3.

Store schema 6 added the typed fact tables for the read families (`task_facts`, `task_processes`,
`task_prompts`, `artifact_manifests`, `artifact_items`) beside the unchanged `task_dataset`,
`task_headers`, `event_streams` and `event_records`; there is no migration, and a schema 5 root was
refused as `unsupported_layout`. The `task_headers` header and status blobs remain the served bytes.
The fixture writers (`add_task_fixture`, `damage_task_fixture`, `append_prompt_blob_fixture`,
`replace_artifacts_fixture`, `append_event_record_fixture`, `truncate_event_log_fixture`,
`remove_event_log_fixture`, `replace_launch_fixture`, `put_launch_attempt`, `put_chain_stop`,
`remove_chain_stop`) are internal to hf-storage's fixture surface and unreachable from
`harflow-ctl`. `hf_interfaces::fixture_port` owns all JSON of the E1 conformance fixture port, and
the private tool `tools/hf-fixture-seed` (`hf-fixture-seed OP --root ROOT`, one JSON object on
stdin, one JSON value on stdout) implements its twenty operations over typed values (the last three, `stop_chain`, `add_launch` and
`add_invalid_launch_name`, write chain-stop markers and pre-record launch receipts).
Chain-root rule of `add_task` and `seed_world`: an explicit `chain_root` (null or text) is stored as given and
needs no predecessor; otherwise it is the predecessor's stored root, else the predecessor id, and a missing or damaged
predecessor refuses the operation (a predecessor named elsewhere in the same `seed_world` counts as present). The
explicit form is a superset of the Python adapter, which refuses it for a missing predecessor. It depends on
hf-domain, hf-application, hf-storage, hf-platform and hf-interfaces, adds no direct `serde_json` or
`sha2` edge, is not a dependency of hf-host and publishes nothing. The five `legacy_*` caller identity
states answer `{"supported": false}`. `hf_domain::timestamp` parses ISO 8601 instants with an offset
to microseconds and refuses naive values.


The internal [archive recovery checkpoint](migration-receipts/E2-B4-archive.md)
rechecks a confirmed inventory under the original maintenance exclusion, publishes
pending before its journal, and archives through BackedUp. Journaled retries use
recorded process identities without reopening SQLite. The macOS adapter uses the
exact approved rustix wrapper for descriptor-relative atomic no-overwrite moves;
file flushes and directory barriers remain explicit and fault-injectable. This
checkpoint remains available as an internal archive helper. The subsequent
[staged completion slice](migration-receipts/E2-B5-completion.md) initializes a
fresh store under the original maintenance exclusion, publishes its identity last,
then commits and reconciles a completed receipt. It adds no public ctl caller.

The public [B6 recovery composition](migration-receipts/E2-B6-public-recovery.md)
checks layout 2 before confirmation, qualifies lease-free historical receipts before
maintenance acquisition, and preserves admitted journal progress in typed application
failures. The host names only the approved MaintenanceSession recovery seam; its
initializer remains tied to the held session. No mutable Store is exposed to ctl.

The [task runtime transaction core](task-runtime-core.md) introduced private T0
intents, atomic observed-spawn T2 publication, separately admitted acceptance and
typed terminal projection. The experimental [first-run source checkpoint](migration-receipts/E2-first-run-source.md)
now composes public run admission with inherited same-binary original owner/writer
channels, raw-source admission, T2/T3 capture publication and T5 terminal projection.
Bounded successful root result text is display-only and must match committed
result evidence before terminal status projection; it never supplies execution
authority. Cancellation remains unavailable, no full-control acceptance is
recorded, and no execution capability is promoted. The first normal release fake
completion attempt refused at caller admission before creating its state root;
actual task completion, caller-anchor qualification, lifecycle acceptance and
production adoption remain unproved.

The caller observer now uses one injected evidence-source algorithm for production
and deterministic ancestry checks. Its shared deadline brackets every boot and
process-record provider call; a complete 128-link walk and exact re-observation
uses at most 258 record-provider calls and 516 boot-provider calls, without a
kernel-syscall count claim. Init remains a distinct root-owned anchor, caller UID
uses real UID, and root-owned ancestors are permitted. The explicitly selected
`MacOsArm64Sdk26_5` decoder is a pure 648-byte observational decoder for the
separately checked SDK 26.5 layout on macOS 26.6.2 arm64. Production reads every
ancestry link, init included, from `kern.proc.pid` through hf-sys `read_into` and
decodes it, on a host whose architecture, `kern.osrelease` and `kern.osversion` are in
a reviewed allowlist (currently aarch64, 25.6.0, 25G83), checked on every read (owner
decision 2026-11-18, coordinator session, option A). Any other host or OS update makes
every caller observation refuse until that build is qualified by a reviewed change.
There is no libproc fallback; libproc refuses root-owned non-init processes for an
unprivileged caller, which would refuse Terminal and SSH sessions. Chain identities are
compared elsewhere with libproc-derived identities, so the kernel record must equal
libproc on pid, parent, effective uid and start time wherever libproc can read the
process; tests sample that invariant. `writer_link`, `owner_entry` and recovery keep
libproc for their own reads. See the [E2-C S1 receipt](migration-receipts/E2-C-S1-init-reader.md). No socket generation correlation, cancellation authority, runtime
admission or external reader adoption follows from this seam.
