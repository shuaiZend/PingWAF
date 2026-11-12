# Quality Patrol — E2E Testing and Design Review

Two independent patrol agents look for defects on a cadence, not because
someone filed a task. This document defines when they run, what they look at,
how a finding is written down, and where it is tracked. It is the contract the
agent role files (`.codebuddy/agents/pingwaf-e2e-patrol.md`,
`.codebuddy/agents/pingwaf-design-review.md`) implement.

Companion material: [e2e-test-plan.md](./e2e-test-plan.md) (scenarios,
environments, acceptance gates), [api.md](./api.md) (contract surface),
[deployment.md](./deployment.md) (upgrade and rollback), `CODEBUDDY.md`
(collaboration protocol).

## 1. Purpose and non-goals

**Purpose**: find the defects that a passing CI run does not
find — behaviour that is wrong only in combination, on a boundary, under a
failure, or in the shape of the code itself.

**Non-goal**: replacing CI. Deterministic gates (`make lint`, `make test`,
`npx tsc -b && npx vitest run`, `make check-docs`, the security audit) already
run on every push and must stay green. A patrol that reports "tests fail" has
found nothing; it has read the CI dashboard.

**Non-goal**: fixing. Both patrol agents are read-only on `src/`. A patrol
agent that edits code has lost its independence and its findings become
self-certified.

## 2. The two agents

| | E2E patrol | Design review |
|---|---|---|
| Role file | `pingwaf-e2e-patrol.md` | `pingwaf-design-review.md` |
| Question | Does the product do what it claims, end to end? | Is the code shaped so it can keep doing it? |
| Works on | a running deployment (demo or a disposable stack) + the API/edge | a commit range / working tree |
| Finds | functional defects: wrong behaviour, wrong status, silent no-op, regression | design defects: coupling, missing extension point, duplicated policy, unrepresentable state |
| Evidence | request, response, event, console state, timestamps | file:line, call path, counter-example that the current shape cannot absorb |
| Never | edits code, edits the deployment config to make a case pass, files a finding it could not reproduce | reports style preferences, files "add tests" as a defect, reviews its own author's changes |

Both are **cadence-driven**: they run on the schedule in §3 even when nobody
asked. Task-driven work still goes to the existing `pingwaf-test` (write and
run the tests a change needs) and `pingwaf-reviewer` (review the change before
merge) roles — the patrol agents exist for what those two miss when nobody is
looking.

The role files live under `.codebuddy/agents/`, which is **gitignored local IDE
tooling** — a fresh clone will not have them. This document is the committed
contract: the table above is the mandate a role file has to implement, so any
teammate can recreate one from it, and a role file that drifts from it is the
one that is wrong.

## 3. Trigger matrix

| ID | Trigger | Who starts it | Scope | Budget | Blocking | Output |
|---|---|---|---|---|---|---|
| T0 | **Pre-commit** — the author is about to commit | author (self) | staged diff only; `docs/e2e-test-plan.md` smoke scenarios if the diff touches enforcement, routing, auth or config defaults | 10 min | yes, self-imposed | inline findings; a Blocker stops the commit |
| T1 | **Pre-merge** — every push to `main` and every release candidate | Tech Lead (or CI on failure) | full diff vs the merge base; the §4 dimensions that the diff touches; `docs/e2e-test-plan.md` §5.1 core regression | 60 min | yes | review note on the change + defect issues for anything High and above |
| T2 | **Weekly patrol** — Monday, on `main` | scheduled (Tech Lead confirms) | the week's merged commits **plus** one full E2E pass on the demo deployment | half a day | no (tracked, not blocking) | weekly digest + one issue per Blocker/High |
| T3 | **Release gate** — before a tag | Tech Lead | full `docs/e2e-test-plan.md` pass + the whole §4 checklist on the release diff | one day | yes | the release report's quality section |

Rules that apply to every row:

1. T1 and T3 may use the same agent run, but the *scope* differs: T1 reviews
   the diff, T3 reviews the release as a whole. Do not merge the reports.
2. A patrol run that finds nothing must still publish its scope ("reviewed
   commit range X, scenarios Y, no findings"), so "no findings" is
   distinguishable from "did not run".
3. **CI cannot start an agent**, so T1 and T3 are enforced by the Tech Lead's
   release checklist, not by a workflow. CI's job is the deterministic half
   (§1); a green CI run is the precondition for a patrol, never a substitute
   for it.
4. T2 never blocks `main`. Its purpose is to surface drift; a Blocker found by
   T2 becomes a normal issue and follows the normal fix path.

## 4. Check dimensions

### 4.1 Functional correctness

| Check | What the agent must show |
|---|---|
| Stated behaviour | the scenario passes with the documented status, body and side effects |
| Negative path | the nearest bypass of every positive case fails closed |
| State settlement | console state, API representation and data-plane behaviour agree within one heartbeat interval |
| Regression | the same operation behaves identically on the previous release unless a changelog entry says otherwise |
| Silent no-op | a configuration that is accepted but changes nothing is a defect, not a documentation gap |

### 4.2 Boundaries and exception paths

Required cases, per unit under test. Missing coverage is itself a finding
(type: functional, severity by blast radius):

| Class | Examples the agent must try |
|---|---|
| Empty / absent | empty list, `null` optional field, missing header, site with no rules |
| Size | 0 / 1 / maximum / maximum+1: rules per site, path length, header size, body limit, batch size |
| Encoding | unicode, percent-encoded, `//`, `..`, `;`, mixed case, trailing slash — for anything that matches on a path or host |
| Duplicate | same upstream in one pool, same domain on two sites, repeated idempotency key |
| Concurrency | two writers on one object, config push racing a read, restart during a push |
| Partial failure | one agent of two unreachable, DB error mid-transaction, upstream timeout, certificate missing |
| Rollback | the operation is reversible, and the reversal restores the previous observable state |
| Degraded mode | control plane down, agent offline, cache empty, disk full — the documented fail-open/fail-closed policy must hold |

### 4.3 Standards and maintainability

- The change follows the surrounding code's idioms; a new pattern needs a
  reason written in a comment, not just a preference.
- No dead code, no commented-out block, no duplicated helper that already
  exists elsewhere (the agent must name where it exists).
- Error messages are actionable: they name the object, the value and the fix
  (compare the capability-gate message in `docs/api.md`, which names the
  agent, its version and the minimum).
- Logs are at the right level and carry the identifiers needed to join them
  with a request (`request_id`, `agent_id`, `site_id`).
- User-visible strings exist in all three locales; documentation that the
  change invalidates is updated in the same change.

### 4.4 Coupling and extensibility (design review's focus)

| Signal | Why it is a defect |
|---|---|
| A new variant of an enum/union handled by `_ =>` in more than one place | the next variant silently takes the wrong branch; the compiler cannot help |
| The same policy implemented twice (e.g. "which layers `*` expands to") | the two copies drift; one of them is wrong the first time the policy changes |
| A module reaching into another module's database rows instead of its API | a schema change becomes a cross-module change |
| An extension point that requires editing N call sites (add a layer, a matcher type, an event type) | cost of the next feature is proportional to the number of existing features |
| A validation that lives only in the UI or only in one of several write paths | the API accepts what the product says is invalid |
| A version-dependent capability with no negotiation | the fleet silently runs two semantics (the 0.32.16 path-scope bypass is the reference case) |
| State that can be represented but not handled (empty string vs absent, both meaning "unset") | every reader needs to guess |

A design finding must include the counter-example: *"add a fifth matcher type
and this function compiles but silently treats it as prefix — see the four
places that match on `match_type`"*. Without it, the finding is a style note.

## 5. Defect record format

One record per defect, identical for both agents. The GitHub issue is the
tracking record (`.github/ISSUE_TEMPLATE/defect.yml` renders the same fields);
the local report is the working evidence.

| Field | Content | Required |
|---|---|---|
| `id` | `QR-<date>-<seq>` (local), plus the issue number once filed | yes |
| `type` | `functional` or `design` | yes |
| `severity` | `Blocker` / `High` / `Medium` / `Low` (§5.1) | yes |
| `title` | one line, states the wrong behaviour, not the file | yes |
| `location` | the narrowest stable pointer: `file:line`, endpoint + method, or scenario ID | yes |
| `trigger` | reproduction steps or the input class that triggers it; for a design defect, the counter-example | yes |
| `observed` / `expected` | what happened vs what the contract says | yes |
| `evidence` | request/response, event id, log line, screenshot path, commit range | yes |
| `fix suggestion` | the minimal change and the test that would catch it; may be "revert X" | yes |
| `blast radius` | who is affected: all sites, mixed-version fleets, IPv6-only hosts, one endpoint | yes |
| `found by` / `at` | `e2e-patrol` or `design-review`, plus trigger ID (T0–T3) and timestamp | yes |
| `acceptance` | how a reviewer will confirm the fix (observable assertion) | yes |

### 5.1 Severity rubric

| Level | Functional | Design |
|---|---|---|
| Blocker | bypass, fail-open on an auth layer, data loss, crash loop, upgrade that cannot complete | a shape that makes the current defect class inevitable on the next change (e.g. two copies of an enforcement policy) |
| High | a primary scenario fails, console and data plane disagree, a documented capability is inert | missing extension point that forces cross-module edits for a routine change; validation absent on a write path |
| Medium | degraded capability with a workaround, wrong status code, misleading message | duplication that has not diverged yet; enum handled by `_ =>` in one place |
| Low | wording, ordering, cosmetic | naming, comment, local style |

Mapping to the review skill's traffic light: 🔴 = Blocker/High, 🟡 = Medium,
🔵 = Low. A design finding is never filed as "add tests" or "consider
refactoring" — those are not defects.

### 5.2 Reproduction-first rule

Adopted from the TDD discipline: **no functional defect without a RED**.

1. Reproduce with the smallest request or scenario that fails, and record the
   exact command plus its output.
2. Assert the observable behaviour (status, body, event, console text) — not
   an internal call. A test that breaks on refactor without a behaviour change
   was testing the implementation.
3. If it cannot be reproduced in the run's environment (missing credential,
   unreachable node), file it as `Medium`, state the blocker explicitly, and
   do not claim it as confirmed.

## 6. Reporting and tracking

| Artefact | Location | Lifetime |
|---|---|---|
| Patrol run report | `.codebuddy/reports/patrol-<YYYY-MM-DD>.md` (local, not committed) | working record for the run |
| Tracking issue | GitHub issue from `defect.yml`, one per Blocker/High | the only durable record; closed only by a human |
| Weekly digest | the T2 report's summary section, pasted into the tracking issue for the cadence | one per week |
| Release quality section | `.codebuddy/reports/release-<version>-<date>.md` §defects | attached to the release |

Rules:

1. **One defect, one issue.** Two agents that find the same thing cross-link
   rather than duplicate.
2. **Medium/Low stay in the local report** unless they recur across two runs;
   then they are filed, because a repeat is a signal that the team is not
   going to get to it otherwise.
3. **Never close an issue as fixed without the acceptance assertion.** The fix
   commit names the issue; the reviewer runs the acceptance line from the
   record.
4. The weekly digest reports: runs executed, findings by type and severity,
   false positives, open issues older than two weeks, and the metric trend in
   §8.

## 7. Collaboration with human review

| Step | Patrol agent | Human (reviewer / owner) |
|---|---|---|
| Discover | reproduces, writes the record | — |
| Triage | proposes severity + type | confirms or adjusts within one working day; a Blocker/High needs the Tech Lead's agreement |
| Fix | does not fix | implements, or delegates to `pingwaf-rust-waf` / `pingwaf-frontend` |
| Verify | re-runs the reproduction and the neighbouring negative cases | runs the acceptance assertion independently; two humans for a Blocker, neither of them the author |
| Close | — | closes the issue, quoting the verifying command |

- An agent finding is a **claim**, not a verdict. A human must be able to
  reproduce it from the record alone.
- A fix that the agent cannot reproduce as fixed stays open, even if the code
  "obviously" addresses it.
- Disagreement on severity is resolved by blast radius, not by opinion: if the
  reviewer cannot state who is affected, the finding keeps its original level
  and the disagreement is recorded in the issue.

## 8. Metrics

| Metric | Definition | Target |
|---|---|---|
| Defect yield | confirmed defects per run | > 0 on a weekly cadence is healthy; 0 for two consecutive weeks means the scope is too narrow |
| False-positive rate | records closed as "not a defect" / all records | < 10% |
| Reproduction rate | records a second person reproduced / all records | 100% for Blocker/High |
| Time to triage | found → severity confirmed | one working day |
| Reopen rate | verified fixes reopened | < 5% |
| Escape link | defects found in production that a patrol should have caught | each one produces a new scenario or checklist row |

## 9. Adoption checklist

- [ ] `.codebuddy/agents/pingwaf-e2e-patrol.md` and `pingwaf-design-review.md`
      exist and are referenced from `CODEBUDDY.md`.
- [ ] `.github/ISSUE_TEMPLATE/defect.yml` exists and its fields match §5.
- [ ] The T1 hook is wired where merges are decided (CI stays green; the
      patrol supplies the judgement CI cannot).
- [ ] A calendar entry exists for T2 with an owner; the demo deployment is
      reachable and its credentials are in the team's secret store.
- [ ] The first weekly run has produced at least one record in the §5 format,
      even if it is a Low.
