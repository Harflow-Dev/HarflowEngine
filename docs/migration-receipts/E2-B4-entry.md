# E2-B4 entry: process absence observation

Delivered locally on 2026-11-16. This closes the process-identity prerequisite for
the recovery flow; it does not implement or accept B4 recovery, `state-recover`,
or a new public capability.

## Behavior

`MacIdentity::observe` retains qualified matching and mismatching identities.
When identity inspection fails, it reports `Dead` only after a positive-PID,
no-signal presence probe returns typed `ESRCH`, bracketed by valid, normalized,
equal boot UUID observations. Permission failures, successful presence probes,
other errors and missing or changing boot evidence remain `Unknown`.

An unreaped process remains conservatively active or ambiguous. Failure to
inspect a process alone never proves death. Existing signal authority and the
default identity-port implementation are unchanged.

## Evidence

- Before the fix, a positively reaped fixture reported `Unknown`; recovery
  assessment refused it as `AmbiguousOwner`.
- After the fix, all 19 process-primitive integration tests and three private
  classifier tests passed. Cases include reaped owners, live owners, identity
  mismatch, unreaped zombies, recovery assessment/recheck, denied probes and
  missing, malformed or changing boot evidence.
- Final full workspace run: 2,084 passed, zero failed, 63 ignored; 301 summaries.
- Formatting, architecture and legacy-name guards, clippy with warnings denied,
  and 23 Python architecture/legacy-guard tests passed.
- Independent code review closed after two rounds; a second independent reviewer
  also found no actionable defect.

The first sandboxed baseline run could not read the kernel boot UUID. The final
workspace run and real-process regression checks ran outside that sandbox, using
owned fixture children and isolated temporary roots. No provider call, live
engine state, application launch, push or release was performed.
