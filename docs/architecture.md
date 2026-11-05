# Architecture

hf-domain owns pure values; hf-application owns ports and use cases and depends only on domain. hf-storage provides the internal fixture foundation described in [storage foundation](storage-foundation.md); hf-backends remains a reserved adapter seam and hf-platform owns bounded process/channel adapters. hf-interfaces maps transports. hf-host composes adapters. hf-contracts is deliberately empty: canonical schemas remain in HarflowMCP. hf-architecture is build-time tooling only. Backend execution and client-registration adapters remain conceptually distinct. The only binary is harflow-engine-scaffold; its default invocation prints an unsupported diagnostic and reads/writes no engine state. Explicit private fixture modes described below operate only on caller-supplied isolated Rust fixture roots.

Target files below 250 substantive lines and functions below 50 code lines. Enforced production maxima are 400 and 80. Changes must preserve constructor injection, explicit errors and bounded work. No compatibility or migration claim follows from a successful build.

The metadata guard enforces the exact member inventory, approved normal dependency edges (including tooling syn), no dev/build dependencies, inherited workspace lints and all crates/tools Rust file sizes. Clippy enforces 80 code lines per function.

The domain guard parses every .rs file under hf-domain with syn. It rejects standard-library roots except the documented value-oriented modules option/result/cmp/clone/fmt/marker/string/vec/collections/iter/convert/default/mem/primitive. Direct and aliased fs/process/env/time/thread/OS imports are rejected before alias use. Root aliases, glob imports, extern crate, unsafe expressions, macros and attributes other than literal string documentation/built-in derives are rejected. This is conservative syntax enforcement, not semantic effect inference; adding external crates or macro expansion requires explicit policy revision. It does not claim whole-program purity for arbitrary future code.

Future execution must specify detached ownership, crash recovery, mutation uncertainty, bounded work and capability negotiation before implementation. Platform effects stay in hf-platform. Never introduce a second writer for Python-owned engine state.

The domain metadata check rejects build scripts and custom target source roots; only the conventional src/lib.rs library target is accepted. Documentation macros are rejected even inside attributes.

Inline modules gated by the exact #[cfg(test)] attribute are exempt from source purity analysis, allowing ordinary unit-test attributes and assertion macros. Out-of-line test modules and files named tests.rs receive no exemption. Production cfg modules and cfg_attr remain subject to the policy. Keep test-specific Clippy allowances inside the inline test module; production denies remain inherited. Non-domain crate roots allow unwrap/expect/panic only under cfg(test).

Internal task-read values, ports, fixture queries and consumer-only mappings are
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
schema2; current fresh roots use schema5 as described below. No automatic upgrade,
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
or new public capability; R04-B's later policy recommendations are not accepted.
