# E2-B5 internal staged initialization and recovery completion

Internal implementation checkpoint, 2026-11-16. Governing behavior is E2-B plan revision 4,
pinned HarflowMCP a891388, following the B4 archive checkpoint 141c6d0. This slice
adds internal R7b initialization and the necessary R8/R9 completion/repeat semantics.
It does not expose state-recover, advertise a capability, publish an engine or
change a client pin. This receipt records local verification; full E2-B and migration acceptance remain open.

## Initialization and exclusion

Storage's files-only initializer takes an existing directory and an injected
identity verifier. It creates no directory, claims no binding and acquires no
lease. It suppresses SQLite checkpoints, configures and initializes the fresh
schema, records identity and sequence, creates the watermark and marker with
directory barriers, then closes the connection before returning. The fixture
initializer retains directory creation, lease acquisition, Binding capture and
final Store open in their original order around the factored files-only call.
A private checkpoint policy preserves the fixture initializer’s original TRUNCATE
behavior; recovery uses FULL to retain its initialized WAL frames. Both paths use
the same ordered implementation and preserve generation consistency.

RecoveryInitializer binds this operation to the original MaintenanceSession and
the journaled staging directory identity, with a ten-second deadline. Platform
effects create the private 0700 staging directory and sync its parent before the
application journals that identity. After initialization returns, the platform
fully flushes the database and WAL and syncs staging. Only the fixed regular file
set is admitted; SHM is discarded without following a link. The journal records
device/inode, length, nanosecond modification time and SHA-256 for all four files.

Replay opens no SQLite database. Each file must exist at exactly one of its
staging/root locations with all recorded attributes and digest matching. Atomic
no-overwrite descriptor-relative publication places store.identity last and syncs
both directories after every rename. Removal of the empty staging directory is
followed by a root barrier. Unknown entries, replaced directories, links and
ambiguous locations refuse without destructive cleanup. Partial initializer
cleanup first qualifies the journaled directory and complete fixed regular file
set; an unjournaled staging directory can only be removed when empty.

## Commit and historical results

Before R8, all original inventory identities, including descendants, must remain in backup/data and none may
remain at the root. Fresh root names are admitted only by their recorded staged
identities. The complete fresh file set, nonempty regular WAL and generation-bound
valid watermark are checked before committing store.recovery.

The committed record is irreversible. A matching committed operation bypasses
archive admission, owner evidence, inventory, staged hash reads and initialization.
Under the original exclusive maintenance session, R9 checks only root/stable lock
and backup/data directory identities, then publishes receipt, manifest and root
journal, reconciling qualified same-operation metadata temporaries. Current store
writers may have changed database, WAL or watermark bytes after the commit.

The separate completed_result API must be called before acquiring maintenance
exclusion. Its bounded read-only adapter reads the selected root's marker and
identity plus the operation's backup manifest and durable receipt. It reads no
current journal or store content and takes no lease, allowing a historical result
to be returned while a Store is open or a later recovery has changed the journal.
It qualifies the exact proposal and backup directory identity as well as the
completed receipt's operation/root/backup path. The Rust canonical codec applies
its own stricter document provenance checks; these are not additional checks
claimed of the pinned Python historical-lookup function.

## Evidence

Final post-fix gate: workspace evidence directory
`.claude-work/codex-2026-11-16/b5-final-gate-r4/` (outside this repository).

- Workspace: 2,172 passed, zero failed, 65 existing ignores, 309 result summaries.
  The two explicit B4 measurements remain ignored here; no new B5 throughput claim follows.
- Formatting, architecture, legacy-name guard, all-target Clippy with warnings denied,
  and both release proof binaries passed.
- Python: 937 tests, OK with two skips and three existing expected failures.
- Pinned conformance: current56/0, intermediate55/1, baseline50/6; the latter two
  retain exactly the B3 failure identities. The unverified capture-loss case remains excluded.
- Strict seed comparison: SAME, zero semantic or unexplained physical differences,
  926 verified allowlisted additions and zero unverified additions.
- Eleven actual-session B5 tests and retained eight B4 tests passed. B5’s63 fault
  boundaries yielded56 successful retries and seven conservative refusals of empty
  publication temporaries; every other boundary must resume. SIGABRT coverage includes
  staging creation before journal, journaled staging, real initializer flush, partial
  cleanup, each publication prefix/barrier and receipt publication. Descendant identity
  loss/replacement/root relocation refuses without deletion.
- Three independent read-only review rounds completed. The last reviewed the narrow
  checkpoint-policy correction after strict seed comparison exposed fixture WAL drift.

Proof binaries and their SHA-256 manifest are retained in that gate’s `bin/` and
`binary-sha256.json`. Earlier r3 evidence failed strict physical parity and is not
this checkpoint’s accepted result. No public recovery command/capability, live-state
operation, client adoption, push or release was performed. B6 remains separate.
