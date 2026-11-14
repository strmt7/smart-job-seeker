# Multi-session fixture readiness - 14 November 2026

The pushed `1caae44923405973ade2567112195880a553d5cc` passed hosted build/test,
coverage, PDF, tool-contract and supply-chain jobs. Its browser job failed:
CI run `43590957669`, job `132435361115`, 138 passed and one failed. The failure
was `fetch_request_stage_can_block_a_loopback_post`: only one of two expected flat
sessions produced an observed paused request. No hosted success for a new commit
is implied by subsequent local checks.

The protocol fixture treated any retained `Page.loadEventFired` as readiness of
the second target. An actual earlier data document plus a delayed second fixture
response reproduces the missing-session assertion locally: one failed native
case at `target/research-recovery-bj3o9k4h/two-target-before.private.log`.
This retains a real earlier load event; no synthetic success or invented HTTP
response is used to make the premise pass.

Readiness now requires the intended flat session's load signal, actual current
URL and completed document state. The event buffer coalesces load signals only
within their own session, so an unrelated target cannot erase another target's
retained signal. A scoped event alone still does not prove a fresh document.
A deterministic cross-session retention regression covers replacement and an
unscoped observation. The existing global event count/byte limits remain.

The native delayed-document case initially passes after the correction, but the
first full local suite still has 138 passes and one failure: the same fixture
observes an additional paused request during servicing. The zero-body fixture
left ancillary icon traffic uncontrolled. Its response now includes an explicit
inline icon, without narrowing the wildcard Fetch patterns or weakening any
request-count assertion. Readiness additionally binds the actual native loader,
including a same-URL reload. The strengthened targeted case passes. The full
current suite is qualified separately; earlier failures remain retained.

The original private controller parsed only successful result groups, producing
zero groups for that failed suite. A separate accounting receipt binds the actual
native 138-pass/1-fail result; no successful suite claim is based on zero groups.

 Both expected
request sessions, one-at-a-time rejection budgets, remaining paused requests,
invalid-request refusal and zero server receipt assertions are preserved.
An initial post-edit compile failed because the test helper omitted its `Value`
import; the corrected compilation and native result are separate retained files.

This fixes a reproducible readiness/qualification failure. Fetch interception
still cannot prevent established WebSocket messages, and this is not a universal
no-send boundary or permission to interact with real employer forms. Public work
remains read-only; interactions remain fictional/offline. The historical failing
CI result and diagnostic limitations remain visible. No detector finding was
manually closed, dismissed or suppressed.

## Final native result

The final fixture-only selection passes **139/139** locally in 372.71 seconds,
with the same four explicit exclusions as CI. The scoped coalescing change is
included in the final 1,414-test workspace run. The strict target/session/loader,
same-URL reload, wildcard Fetch interception and exact pending-budget assertions
all remain. The earlier 138/1 result is not erased or converted to a pass.
The later research-only changes do not alter these browser sources or their
locked dependencies. This is local evidence, not a hosted rerun or universal
cross-engine/transport qualification.
