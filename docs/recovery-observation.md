# Read-only recovery observation (R04-A)

Subsequent [R04-C](migration-receipts/R04-C.md) uses fresh schema5 roots, explicit
lease injection and a safe allowlisted spawn/retained PID waiter. Earlier schema
and close-before-spawn descriptions below record the owning stage; current
concurrent isolation uses dedicated record-lock sentinels, not fixture admission
guards. Schema4 and older roots refuse without conversion.

This is an internal inspection foundation for one explicit operation in an isolated
Rust fixture root. It is not a supported runtime command, recovery service or public
capability. HarflowMCP remains the canonical contract owner; the corrected0.0.10
207-fixture selection and Rust1.99.1 are unchanged. Schema4 was current at R04-A entry.

## Missing production prerequisites

Production recovery still lacks accepted public outcome policy, adoption/control,
owner-longevity and descendant-containment qualification. R04-A therefore observes
existing evidence without creating a recovery intent, journal, claim or grant.
B1's immutable request/reservation/fence and B2c's indexed capture/events suffice for
this subset. They do not authorize takeover. An inspector has no mutation, wait,
signal, cancellation, release, replacement or adoption port.

A visible raw tail does not reveal whether a prior sync completed. Pre-sync and
post-sync tails can look identical. Both remain unindexed with unknown sync
provenance. No inspector advances an offset, finishes capture, repairs a tail or
converts owner absence into another execution right. Active reservations remain
retained for every returned report. Already Released records are explicitly refused;
this slice does not reinterpret historical release policy.

## Composition and interval consistency

`RecoveryInspector` receives separate storage, process-identity and raw-observation
ports. Actual adapter composition is exercised by host integration fixtures; no new
host command or executable/library target is introduced. Domain values are pure
observations, not authority tokens. Alternative injected ports are trusted internal
seams and must supply the same qualified evidence.

One absolute deadline is the lesser of the caller's deadline and start+2seconds.
The inspector reads a qualified operation snapshot, probes the recorded original
owner/child identities, observes qualified raw extents, reads the selected snapshot
again and observes raw extents again. It compares physical source identities,
generation, complete ownership/capture values, event count and the digest of every
validated event; it also compares both raw observations. Any changed bookend is
stale. There is no retry loop, background work or repair after stale/unknown evidence.
Port returns, including errors, are checked against the same deadline.

The report describes an observation interval. Database copies, OS identity samples
and file metadata are not one atomic snapshot, and no report promises that a process
or namespace stays unchanged after its last sample. Busy/unavailable/stale/deadline
results never mean a dead owner or terminal capture. Missing roots/operations return
no record and create no root.

## Source-preserving storage

The read session acquires the existing shared store claim and nonblocking exclusive
I/O gate, copies main+WAL to a private directory outside the source root, and opens
SQLite only on that copy. Recovery and quick-check work occur on the copy. Source
SQLite is never opened/configured, checkpointed or repaired; no source SHM is created
by the inspector. Source binding/marker/deadline are checked around the read and
scratch is cleaned on return. Handles close before identity/raw ports run.

Each main+WAL copy is bounded by64MiB, with a64KiB copy buffer. At most two copies
occur per inspection:128MiB maximum copy work, under the same deadline. This is a
work/admission bound, not a hard wall-clock or RSS guarantee for synchronous filesystem,
kernel or SQLite calls. I/O gate contention may make an inspector or a cooperating
writer return Busy; read-only does not mean contention-free.

Existing ownership/capture readers retain their Store entry points and share a narrow
identity/limits read context for copied reads. Metadata duplicate checks stop at two
rows. Event reads stop at129 rows and refuse beyond128. All eight event scalar
projections use type checks before materialization; malformed large cells are never
decoded as payload. Binding/payload allocation retains the existing preflight limits.

Every selected private event is checked for contiguous revision, monotone offsets,
EOF/loss preservation, no writes after EOF, no no-op progress, valid final exit,
terminal immutability and equality with the capture row. A SHA256 digest covers all
canonical scalar bytes, not only event count/last row. Private events remain internal
capture evidence, not canonical task events. Stale owner/capture binding refuses.

## Identity provenance

The separate macOS arm64 recovery adapter validates and normalizes kernel boot UUID
samples before and after its probe. A full matching PID/start/boot with a known
non-zombie kernel status is reported Present. A matching zombie is reported Zombie;
identity presence is not proof that work is executing. Unknown status is Unknown.

A stable changed boot, a positively observed different start identity at the same PID,
or a positive ESRCH existence probe can establish that the original identity is gone
at that probe. The existence probe sends no signal. Permission errors, failed boot
samples and ambiguous lookups remain Unknown. The original IdentityPort behavior is
unchanged: a missing identity lookup is still Unknown, not death.

These observations supply no retained direct-child waiter or signal authority.
PID reuse never grants control over the new occupant. Original absence supplies
neither an exit code nor pipe EOF, descendant settlement or owner-death release proof.
Boot/PID reuse tests are labeled models; they do not qualify a real reboot or arbitrary
process lifecycle.

## Raw and terminal evidence

Raw inspection uses only the fixed operation-derived stdout/stderr paths. It opens
read-only with nofollow/nonblocking flags and checks regular single-link shape,
expected device/inode, indexed prefix length,65536bytes per-stream maximum and
stable path/open-file metadata. It reads no raw payload and acquires no writer file
lock. Read-only0444 files are valid sources. Missing, replaced, truncated, oversized,
nonregular or unreadable sources cannot qualify a terminal extent.

A report can say RecordedTerminalExtentObserved only when the existing indexed exit,
both EOFs, no loss, complete event chain and exact qualified raw extents agree.
This describes recorded terminal evidence and observed extents; it does not hash or
prove raw content against hostile edits, establish containment, synthesize a public
successful outcome, or grant release/replacement. Incomplete capture and unindexed
tails remain explicit even if the original owner is positively gone.

## Qualification and stop

See [R04-A's receipt](migration-receipts/R04-A.md) for observed fixtures, reviews,
source-preservation checks and fault dispositions. Host test fixtures use bounded
admission before parent descriptor/spawn work; their internal actors and explicit
reader races remain concurrent. This is test isolation, not production synchronization.
Real disk-full/ENOSPC, failed OS
sync and power-loss qualification are excluded: no existing bounded disposable fault
volume is available, and host-wide exhaustion, new installs and privileged setup are
not authorized. Injected failures and safe source read faults are reported separately.

Broader R04 adoption/restart decisions, cancellation/release policy, arbitrary backends
and descendants, hostile writers, cross-platform qualification, R05 and whole R03/R14
acceptance remain outstanding. The Python#112 restart uncertainty remains a regression
obligation, not behavior to copy or an issue closed by this slice. No live root,
installation, registration, sibling app, public contract or tracker is changed.
