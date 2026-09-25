# Critical invariants and boundary cases

## Information disclosure begins at prefill

A live employer page can observe fields as they are entered. User approval is required before populating personal information, not just before the final Submit. Distinct prefill and submit operations use distinct grants. UI approval summaries disclose this boundary.

## Candidate facts and external context

Job requirements and company research cannot create candidate claims. User-attested claims remain attested, not externally verified. Changed evidence invalidates dependent unsent artifacts and approvals; submitted packets remain immutable.

## Transaction state

Persist intent before an external operation. Consume the exact grant atomically. If any remote write might have happened and acknowledgement is uncertain, prohibit automatic retry. A remote system without idempotency cannot support our claim of exactly-once submission.

## Concurrency

One generation and one application browser session by default. A stale task cannot commit over a newer profile/document revision. Use compare-and-swap or transactional revision checks. Cancellation is checked before safe boundaries; after an external-write boundary it creates Uncertain, not fictitious rollback.

## Trust

Models can propose typed data, never permissions. External content is untrusted regardless of its source reputation. Generated text and source quotes require semantic review; neither token stream nor hash proves correctness.

## Resource accounting

Report parent and child processes together. Record dedicated and shared GPU memory, inference offload and display/browser allocations. “Fits 16 GB” must refer to a named workload and hardware/runtime configuration. No hidden cloud fallback.

## Availability and legal constraints

An observed-open listing is time-bounded evidence, not a future guarantee. Sponsorship history, salary estimates and working-language requirements have distinct fields. Missing data is unknown. An assisted tool is not an immigration, payroll or legal decision maker.
