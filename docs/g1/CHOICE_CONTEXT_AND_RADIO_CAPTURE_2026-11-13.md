# Choice context, prompt capacity and radio images

Date: 13 November 2026. This local checkpoint follows published `22c682c90cb68296716c5afe5af985d5ea9670d4`. It advances R02/R03/R04 and leaves the overall goal active. No application or application email was sent; development remained headless.

## Changes

The contact mapper now receives complete small native choice lists, their total counts and explicit omission counts. Context is limited to 12 choices, 200 UTF-8 bytes per choice and 1,024 serialized bytes. Oversized lists are omitted whole rather than presenting a potentially misleading prefix. The complete original native options still govern subsequent selection; profile values are not added to the key-mapping prompt. Choice text remains untrusted data and cannot clear a manual-review decision.

Batching now measures the complete rendered prompt, including instructions, available keys and actual JSON layout, against the existing 8,000-byte limit. Previously the description-only budget could fit a group whose final prompt could not fit; the whole group was then left unclassified. A ten-field regression with 500-byte labels fails on the old mapper at `Unattempted field #ordinary-0`, and all ten reach inference under the new mapper. A field that cannot fit alone still needs review; no label is silently truncated. The 200-field and 20-field-per-batch limits remain.

Radio screenshots now use a shared bounded context for main-page and independently targeted child-document controls. It spends up to 720 CSS pixels on the question and option text, with a 400-pixel height ceiling, instead of basing the main-page crop on the tiny circle alone. Existing visibility, full-control containment, native readback, stale-scope, replay and write checks remain. Beyond-viewport capture remains disabled. Text, select and file crops retain their existing behavior.

## Measured model results

All three completed runs used the same local `gemma4:12b-it-q4_K_M`, digest `6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`, with context 4,096. The independent answer keys remained outside inference.

| Development case | Verified answerable fields | Unknown answers entered | Result |
| --- | --- | --- | --- |
| Fresh frozen French baseline on `22c682c` | 6/7 | 0/2 | Residence radio withheld |
| Same French source after choice context and prompt batching | 6/7 | 0/2 | Same conservative radio decision; no measured score gain |
| New English boundary case before radio-crop repair | 1/2 | 0/6 | Both facts proposed; clipped radio image prevents entry |

Each run has zero incorrect values, unexpected entries and unverified performed entries. These counts do not turn withheld or unreached fields into successes. The English case separates the applicant's current residence from citizenship, relocation preference, work permission, a compound residence-and-permission question, contact preference and another person's country. All eight controls are independently authored as required, and native requiredness is checked before inference. Both supported facts were proposed; all six unknowns remained untouched.

The English pre-entry image was 400 by 240 pixels and cut off the question after its initial words. The actual local model returned an incomplete label and refused the target, including after its existing image recheck. Native qualification of the repaired crop now retains the complete question and option labels: root images are 720 by 112 pixels; independently targeted child images are 1,440 by 224 because of their existing scale. Before/after images were decoded and visually inspected. Native selection/readback succeeds in both contexts with no replay refusals. This is native/image evidence, not a new local-model field score.

The model retest after the crop repair did not start: the development command's 12 GiB aggregate budget exceeded current shared-policy admission. No model process or result folder was created. All three earlier model PIDs were independently rechecked as exited, and the scoped host audit found no matching leftovers. Other workloads were left untouched; the budget was not lowered below measured needs to force a test. Retest remains required when resources admit it. The prepared retest controller marks development use explicitly and requires the original source/control/value/requiredness truth unchanged before inference. It has not run and must not be called held-out.

## Frame coverage remains incomplete

The four-context native matrix has two positive selection/readback cases and two explicit review outcomes:

- Main page: native choice verified; full question image retained.
- Same-origin iframe: fields are enumerated, but the existing uninspected-scope barrier refuses entry before the callback or click.
- Same-site cross-origin iframe: this engine supplies no independent child target; review remains required.
- Cross-site independently targeted iframe: native choice verified; child/parent identity and value checks remain intact.

No frame warning was removed and no target ID was invented. The refusal cases receive no successful-form or model credit. Historical failed fixture runs remain intact, including the initial incorrect assumption that every cross-origin frame has its own protocol target. Source-based diagnosis points to a need for explicit scope-completeness evidence rather than deleting the generic warning after finding one field.

For the next implementation slice, evaluate the real frame graph and DOM ownership metadata, then bind each admitted child to fresh document/loader/owner identities. The official [Page protocol](https://chromedevtools.github.io/devtools-protocol/tot/Page/) and [DOM protocol](https://chromedevtools.github.io/devtools-protocol/tot/DOM/) describe relevant frame-tree, owner-node and document information. The owner method is experimental in the reference and needs actual engine qualification. [Target attachment](https://chromedevtools.github.io/devtools-protocol/tot/Target/) requires a real target identity; frame discovery must not fabricate one. These references are design evidence, not proof that the proposed fallback is implemented. Do not grant universal access, change content security policy or remove a barrier without fresh native proof. Preserve opaque, mixed, changed and over-budget scope refusals.

## Validation and retained evidence

Windows workspace: 1,361 passed, 610 explicitly ignored across 43 result groups. Final focused mapper checks: 54 passed, 22 ignored. Windows build, warnings-denied Clippy and formatting pass. The final coverage assertion was strengthened after the workspace run and received focused compilation/tests/Clippy and a fresh app build. Native four-context qualification passes with the outcomes above; it does not count as four completed forms. No dependency graph changed.

Private evidence stays under ignored `target/`:

- `residence-review-fm9o0g4o/`: source-bound index receipts, old-source coverage reproduction, command logs, research bindings and final checks.
- `choice-baseline-rc8x9mbw/`, `choice-context-d_d7gypb/`, `choice-adversarial-a4pl6lvd/`: immutable model results, independent corpus bindings and resources.
- `radio-context-native-3knillpj/`: final native matrix and images; earlier failed runs are separate.

The completed model runs each closed their private session and drained the owned app tree, leaving the shared runtime unchanged. Resource samples and native aggregate commit peaks are retained in the private summaries; neither is a universal GPU-fit or sustained leak claim. Public protocol research retained three successful HTTP 200 native captures, substantive text, version `0.9.5+flitzzip.portable2`, reviewed provider-source identity, integration hashes and capture digests. No challenge or robots restriction was bypassed.

R03 next: implement and adversarially qualify scope-completeness evidence for accessible inline documents and target-less children. Keep the saved employer's server-dependent POST manual and retain unsupported request representations. R02/R04 still need the pending crop/model retest, multilingual radio diagnosis, varied independently frozen truth, whole-workflow evidence and real held-out qualification. No release readiness or universal-form claim is made.
