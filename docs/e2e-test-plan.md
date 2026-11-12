# PingWAF End-to-End Test Plan

- Applies to: every release from `0.32.23` onward (the plan is versioned with
  the product; a change to scope re-issues this document)
- Scope owner: QA / E2E test engineer; approval: Tech Lead
- Companion documents: [waf-test-plan.md](./waf-test-plan.md) (engine-level
  methodology and dataset), [waf-test-report.md](./waf-test-report.md) (measured
  engine results), [api.md](./api.md) (contract surface), [deployment.md](./deployment.md)
  (environment and upgrade procedures)

This plan covers the product as a whole — control plane, data plane, console and
the wire between them — rather than the detection engine alone. It is written to
be executed as-is: every section states what to run, who runs it, and what has
to exist before the section can be marked done.

## 0. Roles

Referenced by code in the rest of the document.

| Code | Role | Owns |
|---|---|---|
| TL | Tech Lead | scope, gate decisions, escalation, final sign-off |
| QA | E2E test engineer | plan, case design, execution, defect records, report |
| RS | Rust backend / WAF engineer | control plane, engine, agent fixes; root-cause analysis |
| FE | Frontend engineer | console fixes, UI/UE findings, i18n |
| SE | Security reviewer | bypass and authorization findings; independent verification |
| OP | SRE / release engineer | environments, deployments, upgrades, rollback drills |
| DOC | Documentation engineer | user-facing docs, changelog, runbooks kept in sync |

Rule: the person who implements a fix never signs it off. QA verifies behaviour,
SE verifies anything that could weaken enforcement, TL accepts the result.

## 1. Objectives and scope

### 1.1 Objectives

1. Prove that a release can be operated end to end by an operator who has only
   the console and the documentation — from empty database to enforced traffic.
2. Prove that the guarantees the product advertises hold under adversarial input:
   no rule bypass, no fail-open on an authentication layer, no silent loss of
   protection after a config push fails.
3. Prove that the console is usable, not merely functional: the operator can see
   what is enforced, why, and what changed.
4. Produce evidence that survives an audit: reproducible steps, timestamps,
   request IDs, event IDs, and the build under test.

### 1.2 In scope

| Area | Covered by this plan |
|---|---|
| Deployment & lifecycle | fresh install, upgrade from the previous release, rollback, backup/restore |
| Identity | first sign-in, password change, session expiry, role separation (admin / operator / viewer) |
| Sites | create, clone, upstreams/pools/routes, TLS (self-signed, uploaded, ACME), unmatched-host behaviour |
| Rules | managed + custom rules, modes (off/monitor/block), groups, priorities, expression builder |
| Path exceptions | four match types, `skip_layers` semantics, canonical-path behaviour, enable/disable |
| Enforcement | block, challenge, allow, rate limit, IP/geo access, auto-ban and release |
| Distribution | rule push to agents, hot reload, offline cache, reconnect after control-plane restart |
| Observability | access/security events, dashboard counters, fleet view, warnings, version visibility |
| Upgrade | control-plane upgrade with migration, agent self-upgrade (opt-in), version drift detection |
| Console UI/UE | visual consistency, interaction usability, responsive adaptation, accessibility (§8.3) |

### 1.3 Out of scope

- Detection-quality measurement against a labelled corpus — that belongs to
  [waf-test-plan.md](./waf-test-plan.md), which owns the dataset and thresholds.
- Load and capacity certification beyond the smoke budget in §1.4.
- Third-party infrastructure (database tuning, cloud networking, CDN behaviour).

### 1.4 Non-functional coverage (minimum, not exhaustive)

| Dimension | What E2E must show | Threshold |
|---|---|---|
| Availability | control-plane restart does not drop data-plane protection | rules keep enforcing from cache; zero pass-through during restart |
| Consistency | console state equals data-plane state after a push | mismatch window under one heartbeat interval |
| Latency | enforcement overhead per request | within the release's declared budget; no regression over the previous release's measured baseline beyond noise |
| Recovery | restore from backup reproduces the fleet | byte-identical config, no orphan rules |
| Security | no unauthenticated path to a privileged endpoint | every new endpoint carries an auth assertion |
| Operability | one operator, console only, no shell | full §1.1 objective 1 flow completes |

### 1.5 Deliverables for this section

| Role | Responsibility | Deliverable |
|---|---|---|
| TL | freeze scope, approve thresholds | signed scope statement in the release ticket |
| QA | turn scope into the scenario inventory of §3 | scenario list with IDs and owners |
| SE | review the security rows of §1.4 and add missing ones | review note attached to the plan |

## 2. Test environment and data preparation

### 2.1 Topologies

Three topologies, each with a purpose; a release is not accepted until all three
have been exercised.

| ID | Topology | Purpose | Persistence |
|---|---|---|---|
| E1 | Single host, all-in-one container | smoke, UI/UE, quick verification | disposable, rebuilt per run |
| E2 | Distributed: one control plane + at least two agents | rule distribution, drift, self-upgrade, failover | long-lived, reset weekly |
| E3 | Upgrade pair: previous release + current release | migration, rollback, version drift | rebuilt per release |

Requirements common to all three: a disposable origin service behind the edge, a
hostname routed to the edge without touching public DNS, and a recorded egress IP
for the test runner (see §2.4).

### 2.2 Environment build steps

1. **Freeze the build.** Record the image digest and the binary checksum under
   test; every later artefact references them.
2. **Provision E1** from the release image with a fresh database volume; run the
   first-boot path so migrations apply from zero, not from a snapshot.
3. **Provision E2** with the control plane reachable over TLS and the agents
   enrolled with distinct API keys; put at least one agent on a host whose
   address differs from what it reports (NAT or a second NIC) so `peer_ip` is
   distinguishable from `ip_address`.
4. **Provision E3** by installing the previous release first, creating state
   (sites, rules, exceptions, certificates), and only then upgrading.
5. **Seed the fixture** described in §2.3 and capture the seed snapshot (row IDs,
   rule IDs, path-exception IDs) into the run record.
6. **Verify readiness** before any scenario: control plane healthy, agents
   online, the fixture hostname resolves to the edge, and a baseline request
   returns the origin's expected body.
7. **Record the environment fingerprint**: build digest, agent versions, kernel
   and container runtime versions, database version, browser versions used for
   the UI pass.

### 2.3 Fixture data set

| Fixture | Content | Why it must exist |
|---|---|---|
| Origin | one static responder with a marker body and an echo endpoint | a passing request must be provable without reading the proxy |
| Site A | HTTPS site, one upstream, one pool, default route | baseline for enforcement paths |
| Site B | same as A but a second site, used for isolation and clone tests | cross-site leakage is a recurring defect class |
| Managed rules | default seeded rule pack, untouched | baseline for mode and override tests |
| Custom rules | one block rule and one allow rule on disjoint paths, with explicit priorities | rule ordering and allow semantics |
| Path exceptions | one entry per match type, plus one that skips every layer | the matrix in §3.1 needs all four |
| Access rules | one IPv4 allow, one IPv4 deny, one geo restriction | IP precedence and auto-ban interplay |
| Certificates | one uploaded certificate, one ACME staging attempt | TLS paths and ACME state machine |
| Fault fixtures | a deliberately broken expression and an unreachable upstream | compile-failure and fail-open behaviour must be testable |

Seed data must be reproducible from the run record alone: a reviewer must be able
to rebuild the same fixture from the record without asking the tester.

### 2.4 State hazards and how to neutralise them

| Hazard | Effect if ignored | Control |
|---|---|---|
| Automatic IP ban (10-minute window) | every later request from the runner returns 403, producing false "regression" reports | allow-list the runner's egress IP before execution, or clear the ban explicitly between phases and re-check the ban list is empty |
| Challenge and rate-limit counters | later scenarios inherit an exhausted budget | per-phase reset, and a fresh path per scenario |
| Config push delay | assertions race the data plane | wait for the observable confirmation (event, applied version, console state), never a fixed sleep alone |
| Cached rules on agents | a "fixed" behaviour still shows the old verdict | force a resync and confirm the applied version before asserting |
| Migration irreversibility | a failed E3 run leaves a database that cannot be reused | snapshot before upgrade; rebuild rather than repair |
| Clock and time zone | expiry, retention and ban windows disagree with the tester's expectations | pin the runner's clock source to the server's, and state the zone in the record |

### 2.5 Deliverables for this section

| Role | Responsibility | Deliverable |
|---|---|---|
| OP | build E1–E3, hand over access and fingerprints | environment sheet per topology (URLs, credentials reference, digests) |
| QA | seed fixture, record seed snapshot, reset procedure | seed record + reset runbook |
| RS | provide the fault fixtures and confirm they fail the intended way | fixture notes |

## 3. Key user scenarios and end-to-end flows

Scenarios are the unit of acceptance. Each one is a business flow, not a feature
check; a scenario passes only when every assertion in its row holds.

### 3.1 Primary flows

| ID | Scenario | Key steps | Assertions |
|---|---|---|---|
| E2E-01 | First boot to protected site | sign in, change password, create site, add upstream, upload certificate, verify traffic, open the dashboard | traffic reaches origin; certificate presented; dashboard counters move; no warning banner |
| E2E-02 | Rule enforced from console to edge | create a block rule on a path, enable it, request the path | console shows the rule enabled; the edge returns the enforcement response; a security event names that rule; access log records the request once |
| E2E-03 | Mode change takes effect | switch the rule to monitor, request the path again | request passes; event still recorded with a monitor verdict; no auto-ban accrues |
| E2E-04 | Path exception exempts a scope | add an exception skipping the WAF for a prefix, request inside and outside the prefix | inside passes with a malicious payload; outside is blocked; the exception hit is logged when logging is on |
| E2E-05 | Exception cannot be escaped | request canonical variants of the exempt path (`..`, encoded, doubled separators, trailing semicolon) | only the intended scope is exempt; every variant that resolves outside it is still blocked |
| E2E-06 | Group and priority semantics | disable one rule group, keep another; reorder two conflicting rules | only the disabled group's rules stop firing; the winner is the lower priority number; managed rules unaffected by group state |
| E2E-07 | Access rules and auto-ban | deny an IP range, trigger repeated blocks, release the ban | deny applies before WAF; ban appears with its expiry; release restores access without restart |
| E2E-08 | Distribution and reconnect | kill the agent, push a rule change, restart the agent | agent reconnects, applies the new rules, and protection resumes; no gap where a blocked path passes |
| E2E-09 | Control-plane restart | restart the control plane while the edge serves traffic | edge keeps enforcing cached rules; console reports agents offline then online; no enforcement gap |
| E2E-10 | Upgrade with migration | run E3: install previous release, create state, upgrade, verify | migrations apply once; all pre-upgrade objects survive; enforcement identical before and after |
| E2E-11 | Rollback | stop the new build, restore the previous binary and the pre-upgrade snapshot | the fleet returns to the previous behaviour; no schema downgrade attempt |
| E2E-12 | Backup and restore | export configuration, wipe a disposable control plane, restore | restored fleet is identical (sites, rules, exceptions, certificates); invalid rows are reported, not silently dropped |
| E2E-13 | Version drift and self-upgrade | enrol an older agent, enable opt-in self-upgrade, publish a newer control plane | console flags the drift; the agent downloads, verifies, replaces, restarts, and the console shows convergence; the previous binary is retained for rollback |
| E2E-14 | Unmatched host and error pages | request a host with no site; request a site with custom error pages | configured error page served; no upstream details leak; the event is attributable |

### 3.2 Supporting flows

Consulted when a primary flow fails, or when a change touches the area:
sign-in failures and lockout, session expiry and refresh, role-restricted views,
site clone, certificate renewal status, cache purge and quota, log retention, AI
assistant and MCP endpoint authorization, API-key lifecycle.

### 3.3 Flow-level rules

1. A scenario starts from a stated state and ends with that state restored, or
   explicitly declared as leaving state behind.
2. Every scenario captures evidence at three layers: the client response, the
   security/access event, and the console rendering. One layer alone is not
   evidence.
3. Any scenario touching enforcement is executed twice: once with the feature
   enabled, once disabled, so the delta is attributable.

### 3.4 Deliverables for this section

| Role | Responsibility | Deliverable |
|---|---|---|
| QA | expand each scenario into executable cases with the assertions above | scenario-to-case map |
| RS | confirm the assertion points exist in events/logs/DB | field reference for each scenario |
| TL | approve the scenario set as the release's acceptance basis | approved list in the release ticket |

## 4. Test case design principles

1. **One variable per case.** The enabled/disabled pair in §3.3 is the only
   permitted way to weaken an assertion.
2. **Assert the observable, not the internal.** Prefer status code, event record,
   applied config version, console text. Never assert "the function was called".
3. **Negative first.** For every positive path, design the nearest bypass: a
   canonical variant, a header case change, a trailing separator, an encoded
   character, an unexpected method. A patch that only satisfies the positive case
   is not done.
4. **Boundary tables beat duplicated cases.** Encode ranges as data (1/2/3 rules,
   empty/long/pathological expressions, 0/large page sizes) and sweep the table
   instead of writing a case per value.
5. **State must be explicit.** Every case declares its preconditions and its
   cleanup. Cases that depend on leftover state are rejected at review.
6. **Attribution requirement.** A failing case must name the layer that failed
   (console, API, distribution, engine, edge). A case that cannot be attributed
   is a bad case, not a mystery to be debugged in the release.
7. **Determinism.** Time-sensitive cases use the control plane's clock and state
   the tolerance; a case that can pass or fail on timing alone is rejected.
8. **Traceability.** Each case maps to a requirement or a documented behaviour;
   a case with no requirement is either a missing requirement or dead weight.
9. **Review gate.** SE reviews every case whose expectation is "allowed" or
   "exempts"; QA reviews the rest. Cases that assert a weakening of enforcement
   need SE's signature before execution.

Deliverables: QA authors the case inventory with the eight attributes above
(ID, requirement, preconditions, steps, assertions, evidence, cleanup, owner);
SE signs the negative and exemption cases; TL samples 10% for coverage.

## 5. Execution strategy and automation

### 5.1 Execution tiers

| Tier | Trigger | Content | Duration target | Blocking |
|---|---|---|---|---|
| Smoke | every pull request and every push to main | sign-in, one site, one rule, one exception, one upgrade path (dry) | minutes | yes |
| Core regression | every push to main, every release candidate | E2E-01…E2E-09 plus the UI/UE checks of §8.3 | under an hour | yes |
| Full pass | release candidate and nightly | all scenarios incl. E2E-10…E2E-14, three topologies, UI/UE full sweep | half a day | yes for release |
| Drill | per release or monthly | rollback, restore, ban release, agent self-upgrade | staged | yes for release that touches those areas |

### 5.2 Automation layers

| Layer | What is automated | What stays manual | Why |
|---|---|---|---|
| L1 | unit and property tests inside the crates | — | fastest signal, owned by engineers |
| L2 | API contract and proto/frontend type parity | — | catches drift the console cannot see |
| L3 | single-service flows (plugin, engine, sender/receiver pair) | — | narrows blame before E2E |
| L4 | browser-driven scenarios and raw edge requests | exploratory pass, visual review | UI/UE judgement needs a human |
| L5 | upgrade, rollback, restore, fleet drift | drill scheduling and interpretation | destructive by nature |

Rules for L4 and L5 automation: raw edge requests are preferred over a browser
when the assertion is about enforcement; the browser is used when the assertion is
about the operator's ability to act. Both must run against the same fixture so a
discrepancy between them is meaningful.

### 5.3 Ordering, parallelism and isolation

1. Run destructive and state-mutating scenarios last within a tier (E2E-10…14).
2. Never parallelise two scenarios that share a site, an egress IP, or a rate-limit
   bucket.
3. Parallelise by site, not by scenario, when the tier must finish faster.
4. A failed scenario does not abort the tier; it is quarantined so dependent
   scenarios report "blocked" rather than "failed".
5. Re-run policy: only for infrastructure failures (environment unavailable,
   network reset), never for a functional assertion. Every re-run is recorded with
   its reason.

### 5.4 Environment occupation and reset

Each tier declares which topology it owns for the window; overlapping claims are
rejected. Between tiers: reset the fixture, clear bans and counters, confirm agent
versions, and re-capture the environment fingerprint.

### 5.5 Deliverables for this section

| Role | Responsibility | Deliverable |
|---|---|---|
| QA | own the tier matrix, ordering rules and run record | execution runbook + per-run record |
| OP | provide isolated topology windows and reset automation | environment calendar, reset job |
| RS/FE | keep L1–L3 suites green and fast | layer health note per release |

## 6. Defect management

### 6.1 Severity (product-specific)

| Severity | Definition | Examples |
|---|---|---|
| Blocker | any bypass, any fail-open on an authentication or denial layer, data loss, crash loop, upgrade that cannot complete | an exception scope that can be escaped; a 401 path that returns content; a migration that drops rules |
| High | a primary flow (E2E-01…09) fails, or console state contradicts the data plane | rule shows enabled but never fires; version card reports the wrong build |
| Medium | a documented capability is degraded with a workaround | pagination truncation without notice; missing hint text |
| Low | cosmetic, wording, focus order | misaligned cell, ambiguous label |

Bypass and authorization findings are always Blocker regardless of who found
them, and they cannot be downgraded by the implementer.

### 6.2 Lifecycle

New → Triage (severity + owner + target) → Fixing → Ready for verification →
Verified (or Reopened) → Closed. Triage within one working day; a reopened
defect that had been closed as fixed is recorded as an escape (see §10).

### 6.3 Reproducibility package

Every defect carries: build digest, topology ID, fixture seed snapshot, exact
request or UI steps, observed vs expected, timestamps, request ID and event ID,
console screenshot or recording for UI findings, and whether the runner's IP was
allowed/blocked at the time. A defect without this package is returned to the
reporter rather than triaged.

### 6.4 Fix verification

1. QA writes the failing case first (or attaches the existing one) and shows it
   failing on the old build.
2. RS/FE fix; QA re-runs the failing case plus the neighbouring negative cases
   (§4, principle 3) on the new build.
3. SE independently re-verifies every Blocker and every enforcement-related fix.
4. TL closes only when both QA and SE have signed.

### 6.5 Deliverables for this section

| Role | Responsibility | Deliverable |
|---|---|---|
| QA | triage, defect records, verification evidence | defect log with the package of §6.3 |
| RS/FE | root-cause note, fix, regression case | fix commit + a case that fails before it |
| SE | independent verification of enforcement fixes | sign-off note |
| TL | severity arbitration, closure | release defect summary |

## 7. Regression strategy

### 7.1 Impact matrix

| Change type | Mandatory regression |
|---|---|
| Rule engine or evaluation order | E2E-02/03/06, engine tier of waf-test-plan, all exemption cases |
| Path matching or exception semantics | E2E-04/05 plus every canonical variant in the matrix |
| Config distribution or proto | E2E-07/08/09, L2 contract suite, version-drift check |
| Auth, session, role, API keys | all sign-in scenarios, one negative per endpoint class |
| Database model or migration | E2E-10/11/12 on E3, restore drill |
| Listener, TLS, port binding | E2E-01/14, dual-stack and container checks |
| Console component or i18n | UI/UE sweep of §8.3 in all three languages |
| Dependency or base-image change | smoke tier on all topologies, plus the security audit gate |

### 7.2 Regression set composition

The minimum set is the union of: everything in the impact matrix for this
release's changes, every scenario containing a Blocker found in the last two
releases, and every case that has failed at least once in the last three runs.
This set is regenerated per release, never carried by habit.

### 7.3 Time- and state-dependent checks

Cases involving ban windows, retention, certificate expiry, config versioning,
and cache TTL require either a controllable clock or a documented tolerance. The
tolerance is stated in the case; if a case cannot state it, it is not regression-safe
and belongs in the manual drill.

### 7.4 Deliverables

QA publishes the per-release regression set with justification per row; TL
approves the omissions; OP confirms the drill window.

## 8. Pass criteria and acceptance

### 8.1 Release gate

| Gate | Criterion |
|---|---|
| Blockers | zero open |
| High | zero open; a deferral needs TL + SE written approval and a dated follow-up |
| Primary scenarios | E2E-01…09 at 100% pass on all three topologies |
| Full pass | ≥ 98% of executed cases pass, with every failure attributed to a defect or an accepted exclusion |
| Regression | the §7 set at 100% pass |
| Upgrade | E2E-10…12 pass, with the rollback drill recorded |
| Security | no endpoint without an authentication assertion; no fail-open finding |
| UI/UE | §8.3 all four dimensions pass |
| Evidence | run record, defect log, environment fingerprints, and the report of §10 are attached to the release ticket |

### 8.2 Consistency gate

For every configuration change asserted in E2E-02…09, the console state, the API
representation, and the data-plane behaviour must agree within one heartbeat
interval. A settlement that requires a manual refresh is a Medium finding at
minimum.

### 8.3 UI/UE acceptance

Each dimension is judged on a sampled set (at minimum: the site list, the rules
panel with dialog open, the path-exception panel, the observability page, and one
error state per page), in all three languages, at three widths (desktop, tablet,
narrow viewport).

| Dimension | Verification points | Pass criterion |
|---|---|---|
| Visual consistency | one primary action per view; spacing, type scale and colour roles come from the shared set; icons and badges mean the same thing on every page; empty, loading and error states look deliberate; no mixed-language text | no unexplained variant of a shared component; no raw/unstyled element; no untranslated key visible; screenshot review signed by FE and QA |
| Interaction usability | every destructive action confirms and states its blast radius; every asynchronous action shows progress and a terminal state; errors are visible without scrolling to find them; the primary task is reachable within the first screen; tables never hide the actions column behind a horizontal scroll at the sampled widths; focus returns to a sensible place after a dialog closes | the reviewer can complete each sampled task without asking how; no action silently does nothing; no error rendered outside the viewport; keyboard-only completion of the primary task |
| Responsive adaptation | layout reflows without content loss at all three widths; no clipping of labels, badges or buttons; tables degrade predictably (truncation with full value available, not disappearance); dialogs remain operable at the narrow width | zero horizontal overflow of the page shell; zero inaccessible action; text remains readable without zoom |
| Accessibility | every control reachable and operable by keyboard with a visible focus ring; dialogs trap and restore focus; form fields have labels, and errors are announced; icon-only buttons have accessible names; colour is never the only carrier of meaning; contrast meets the documented ratio in both themes | primary tasks completable by keyboard alone; automated accessibility scan reports zero critical findings on the sampled set; manual focus-order pass signed by QA |

A UI dimension fails if a defect in it is High or above; Medium findings in UI are
allowed only with a dated follow-up, and the count is reported in §10.

### 8.4 Deliverables

| Role | Responsibility | Deliverable |
|---|---|---|
| QA | assemble gate evidence | gate table with pass/fail per row |
| SE | security gate signature | security sign-off |
| FE | UI/UE review in all languages and widths | UI review note with screenshots |
| TL | accept or reject the release | signed decision in the release ticket |

## 9. Risks and mitigations

| Risk | Signal | Mitigation | Owner |
|---|---|---|---|
| Shared demo environment is mutated by another party mid-run | unexplained 403/404, fixture drift | exclusive window per tier, fingerprint check at start and end, rebuild if drift is detected | OP |
| Automatic bans contaminate later scenarios | mass 403s from the runner | runner allow-listed; ban list asserted empty between phases | QA |
| Config push races the assertion | flapping results | assert on applied version/events, never on elapsed time alone | QA |
| Agent version drift hides a defect | behaviour differs between agents | version parity checked before execution; drift flagged in the fleet view | OP |
| Migration cannot be undone | failed upgrade leaves a dead database | pre-upgrade snapshot, rebuild-not-repair rule, rollback drill per release | OP |
| Self-upgrade replaces the wrong binary or is interrupted | process does not restart, or restarts on the old build | drill in E2, digest pinned, previous binary retained, rollback documented | RS |
| Test data becomes unrepresentative | scenarios pass while real payloads fail | keep the engine corpus in the engine plan; E2E uses it only for spot checks | QA |
| Time-dependent cases flake | pass/fail alternates | controllable clock or documented tolerance, else move to the drill | QA |
| Release pressure trims the full pass | gate rows skipped | gate table requires explicit omission with TL approval; omissions reported | TL |
| Reviewer and implementer are the same person | defects closed without independent check | §6.4 separation enforced in the ticket's sign-off fields | TL |

Each risk is reviewed at the start of a release (TL) and re-checked in the report
with the actual occurrence.

## 10. Reporting and metrics

### 10.1 Report structure

1. Build and environment fingerprint, tier and topology coverage.
2. Results per tier: executed / passed / failed / blocked / not run, with reasons.
3. Defect summary by severity, with the Blocker and High list in full.
4. Scenario matrix (E2E-01…14) with pass/fail and evidence links.
5. UI/UE section: the four dimensions, findings, and the screenshots reviewed.
6. Deviations from the plan, and omissions with approval references.
7. Conclusion: deliverable / deliverable with conditions / not deliverable, with
   the conditions listed and dated.
8. Follow-ups: accepted deferrals, new cases to add, environment fixes.

### 10.2 Metrics

| Metric | Definition | Target |
|---|---|---|
| Scenario pass rate | passed / executed, primary scenarios counted separately | 100% primary, ≥ 98% overall |
| Gate compliance | gate rows satisfied / total | 100%, omissions listed |
| Defect density | defects per scenario, by area | trending down release over release |
| Blocker escape rate | Blockers found after release / all Blockers | zero, tracked per release |
| Fix turnaround | triage to verified, by severity | Blocker same day, High within two days |
| Reopen rate | verified defects reopened / verified defects | under 5% |
| Automation share | executed cases automated at L1–L4 | trending up, never at the expense of the negative coverage in §4 principle 3 |
| Flake rate | cases failing then passing without a code change | under 2%, each instance root-caused |
| Environment incidents | runs lost to environment faults | under 5% of runs |
| UI finding mix | open UI findings by dimension and severity | zero High; Medium listed with dates |

### 10.3 Publication and review

The report is published with the release, retained with the release artefacts, and
reviewed in the next release's kick-off: any metric that moved the wrong way is
either explained or becomes a risk row in §9.

### 10.4 Deliverables

| Role | Responsibility | Deliverable |
|---|---|---|
| QA | write the report, own the metrics | release E2E report + metric table |
| RS/FE | contribute root causes and follow-up estimates | per-defect cause note |
| SE | security section | security verdict |
| TL | conclusion and conditions | signed conclusion in the release ticket |
| DOC | fold durable lessons into user-facing docs | doc delta list |
