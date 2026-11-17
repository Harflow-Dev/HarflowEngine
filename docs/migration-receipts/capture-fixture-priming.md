# Capture poison fixture priming

Date: 2026-11-17. Base: `ce4f1935c1e782601d52acfc8437cce432a4f775`.
Scope: test fixture synchronization and stronger assertions only.

The first full workspace verification stopped at the existing lost-publish-reply
capture test's requirement that both streams contain uncertain bytes. Primary and
two independent source diagnoses found an uncontrolled precondition: independent
nonblocking pipe reads permit a one-sided first batch, while only the first failed
publication becomes uncertain. The final reader-progress wait did not determine
that batch's composition. The caller decoder changes are not on this capture path.

One diagnostic-only focused run observed a valid two-sided first publication and
passed. It was not repeated until failure, and does not establish the tuple from
the earlier failed run. The correction makes the intended precondition explicit.

The optional producer control mode now primes one byte on each pipe before reader
startup. After the parent observes both accepted bytes, a second phase supplies one
more byte per stream. At most three batches are queued before bulk output, below
capacity four. The owner starts only after both phases are accepted; bulk output
is released while capture-begin remains gated. This guarantees both first-batch
uncertainty and later accepted bytes for abandonment without pipe-capacity or
scheduler-order assumptions. Pattern bytes and total output remain unchanged.

Parent waits share the existing owner deadline and remain under the owned child
guard. Child marker waits have finite five-second phase deadlines. The healthy
default producer path stays unprimed. No production capture, caller, dependency,
policy or existing timeout budget changed.

Every prior assertion remains: positivity, raw prefixes, persisted offsets,
per-stream conservation, poisoning and no further mutation. New assertions require
the first submitted offsets to equal uncertainty exactly at `(1, 1)`. The
diagnostic mutex is released before storage calls.

Both focused accounting tests passed after the correction. The final combined
workspace run passed 2,319 tests, with 65 existing ignores and no failures.
Strict workspace Clippy, formatting, architecture and legacy checks passed.
The combined Python verification ran 951 tests successfully, with two skips and
three expected failures.
Independent fixture review cleared ordering, bounded cleanup and assertion scope.
External diagnosis, review and check evidence is retained in the parent workspace
under `.claude-work/codex-2026-11-17/next-mac-rust/`.
