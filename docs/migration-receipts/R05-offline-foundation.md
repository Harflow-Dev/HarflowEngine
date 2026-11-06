# R05 offline Claude foundation receipt

Status: preparatory implementation; aggregate R03/R04/R05 acceptance remains open.
Base: `b05d9338ef6eab911aedce238434195f50a6c74f`.
Scope: HarflowEngine only, synthetic/offline observations, unchanged selected pins.

## Delivered behavior

The [foundation design](../claude-offline-foundation.md) describes declaration
binding, bounded transport parsing, passive session/tool reduction and read-only
owner-policy projection. It adds no process/input/release port and does not alter
HFOP1, storage schemas, canonical contracts or the measured no-tools oracle.
The original caller and detached owner remain separate identities. Backend
results, recorded-input exhaustion and physical settlement remain separate facts.

The offline gate compares the complete declared context and recomputes payload
and invocation digests before one observation call. Live, resume and production
modes refuse before that call. The reducer binds summaries to the full declared
context, isolates root/child tool scopes, preserves explicit failure across later
malformed data, and refuses reused tool identities for the entire attempt.
The decoder preserves complete records within the remaining stream budget even
when the pushed chunk crosses that budget.

Owner evidence supplies exit, EOF and loss. Independent settlement observations
cannot overwrite it. Pending, poisoned, cancelled or mismatched owner evidence
blocks successful completion. Unknown containment can still coexist with an
observed successful workflow but blocks release evidence; no observation creates
a release grant.

## Review and verification

The independent plan review required physical evidence precedence, separate
completion/revision domains and full scoped tool correlation. Final source review
found resolved-tool ID reuse and loss of a valid terminal prefix at stream budget;
both were accepted for correction with dedicated regressions. Dedicated test
ownership is separate from production implementation. The final review also
corrected synthetic settled capture progress from revision 1 to reachable revision
2, still below owner fence revision 3. All accepted findings are resolved.

Final checks on the completed source:

- Locked Rust workspace: 398 passed, zero failed, 17 ignored; includes all 20 new
  offline tests and real-SHA synthetic host composition.
- Formatting, locked check, all-target Clippy with warnings denied, architecture,
  legacy-name and whitespace guards passed.
- All 22 Python checks passed.
- The default scaffold reports engine operations unsupported.

No vendor/provider test was enabled. Local results do not establish hosted CI or
other-platform qualification. The pre-existing tracker-file diff remained
byte-identical throughout this stage.

## Remaining gates

Actual Claude owner launch needs an explicitly accepted preparation/assignment
contract. Caller/grant authentication, exclusive production roots, runtime closure,
OS enforcement, tool/live-input/resume, containment, resource budgets and
fault/recovery qualification remain outstanding. This receipt does not accept the
whole R03/R04/R05 stage or select a client runtime. No vendor/provider probe,
credential use, install, registration, acquisition, push or release occurred in
this offline stage. Existing isolated protocol measurements are preserved rather
than rerun.
