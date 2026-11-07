# Architecture

hf-domain owns pure values; hf-application owns ports and use cases and depends only on domain. hf-storage provides the internal fixture foundation described in [storage foundation](storage-foundation.md); hf-backends contains diagnostic fixture construction without a production runtime and hf-platform owns bounded process/channel adapters. hf-interfaces maps transports. hf-host composes adapters. hf-contracts is deliberately empty: canonical schemas remain in HarflowMCP. hf-architecture is build-time tooling only. Backend execution and client-registration adapters remain conceptually distinct. The binaries are harflow-engine-scaffold and harflow-ctl (hf-host) and the private tools hf-architecture and hf-fixture-seed. The scaffold's default invocation prints an unsupported diagnostic and reads/writes no engine state. harflow-ctl is the public read-only surface: `version-info` and `state-inspect` (a lease-free observer that opens no SQLite database and never creates anything) everywhere, and on macOS arm64 the store-backed reads `list`, `status`, `status-batch` and `task-list-page` (including successor discovery). The reads classify the root with the same observer first, open the store only through the read session under a shared lease, run each command inside one snapshot and create nothing, including for a missing root. Authority-gated reads apply the caller gate of `hf_domain::ctl::caller`, whose two named switches (`REFUSE_UNRESOLVED_AGENTS`, `MISSING_ROOT_READS_EMPTY`) are the only places the owner decisions on unresolved agents and missing roots are encoded; the admitted class is `NoAgentDetected`, never a human credential. harflow-ctl advertises `ctl.v1` and `version-info.v1`, plus `task-successors.v1` on macOS arm64, and answers every other path with `invalid choice`, exit 2. Its code lives in a `ctl` module per layer and may not name the private fixture modes below. R14's target is macOS arm64; Linux is W02 and Windows WIN02, so CI there covers platform-independent code only.  Explicit private fixture modes described below operate only on caller-supplied isolated Rust fixture roots.

Target files below 250 substantive lines and functions below 50 code lines. Enforced production maxima are 400 and 80. Changes must preserve constructor injection, explicit errors and bounded work. No compatibility or migration claim follows from a successful build.

The metadata guard enforces the exact member inventory, approved normal dependency edges (including tooling syn), no dev/build dependencies, inherited workspace lints and all crates/tools Rust file sizes. Clippy enforces 80 code lines per function.

The domain guard parses every .rs file under hf-domain with syn. It rejects standard-library roots except the documented value-oriented modules option/result/cmp/clone/fmt/marker/string/vec/boxed/collections/iter/convert/default/mem/primitive. Direct and aliased fs/process/env/time/thread/OS imports are rejected before alias use. Root aliases, glob imports, extern crate, unsafe expressions, macros and attributes other than literal string documentation/built-in derives are rejected. This is conservative syntax enforcement, not semantic effect inference; adding external crates or macro expansion requires explicit policy revision. It does not claim whole-program purity for arbitrary future code.

Future execution must specify detached ownership, crash recovery, mutation uncertainty, bounded work and capability negotiation before implementation. Platform effects stay in hf-platform. Never introduce a second writer for Python-owned engine state.

The domain metadata check rejects build scripts and custom target source roots; only the conventional src/lib.rs library target is accepted. Documentation macros are rejected even inside attributes.

Inline modules gated by the exact #[cfg(test)] attribute are exempt from source purity analysis, allowing ordinary unit-test attributes and assertion macros. Out-of-line test modules and files named tests.rs receive no exemption. Production cfg modules and cfg_attr remain subject to the policy. Keep test-specific Clippy allowances inside the inline test module; production denies remain inherited. Non-domain crate roots allow unwrap/expect/panic only under cfg(test).

hf-storage also provides a read session that answers task and event reads from the real store read-only under a shared lease; it is specified in the [read-path design](store-read-path.md) and the [storage protocol](storage-protocol.md) and adds no ctl command. Internal task-read values, ports, fixture queries and consumer-only mappings are
described in [task reads](task-reads.md). They add no supported host command or
production caller/lifecycle verification.

Internal stored event paging and consumer-only mappings are described in
[event reads](event-reads.md). They retain fixture authority and observation
boundaries; that stage added no public host operation or watcher.

The macOS arm64 fixture-only [process primitives](process-primitives.md) retain
qualified direct-child handles and independent capture/exit evidence. They add no
detached owner, public execution capability or descendant-containment guarantee.

The [durable ownership foundation](ownership-foundation.md) adds isolated internal
intent/reservation/claim transactions and fenced observations. The original B1 stage selected
schema2; current fresh roots use schema6 as described in the fixture seed paragraph below. No automatic upgrade,
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

Store schema 6 adds the typed fact tables for the read families (`task_facts`, `task_processes`,
`task_prompts`, `artifact_manifests`, `artifact_items`) beside the unchanged `task_dataset`,
`task_headers`, `event_streams` and `event_records`; there is no migration, and a schema 5 root is
refused as `unsupported_layout`. The `task_headers` header and status blobs remain the served bytes.
The fixture writers (`add_task_fixture`, `damage_task_fixture`, `append_prompt_blob_fixture`,
`replace_artifacts_fixture`, `append_event_record_fixture`, `truncate_event_log_fixture`,
`remove_event_log_fixture`) are internal to hf-storage's fixture surface and unreachable from
`harflow-ctl`. `hf_interfaces::fixture_port` owns all JSON of the E1 conformance fixture port, and
the private tool `tools/hf-fixture-seed` (`hf-fixture-seed OP --root ROOT`, one JSON object on
stdin, one JSON value on stdout) implements its seventeen operations over typed values. It depends on
hf-domain, hf-application, hf-storage, hf-platform and hf-interfaces, adds no direct `serde_json` or
`sha2` edge, is not a dependency of hf-host and publishes nothing. The five `legacy_*` caller identity
states answer `{"supported": false}`. `hf_domain::timestamp` parses ISO 8601 instants with an offset
to microseconds and refuses naive values.
