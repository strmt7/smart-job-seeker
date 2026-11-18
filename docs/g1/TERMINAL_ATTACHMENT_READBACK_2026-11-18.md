# Attachment verification at final review · 18 November 2026

The saved-page rehearsal now preserves the verified file input's native
identity, owning form, document and scoped field label across sections.
When both the browser summary and the independent image read agree on a
filename, it checks the current selection from that original input against
the prepared PDF. The staged file and the bytes retained by Chromium must
each match the prepared digest.

The selections are hashed in one isolated browser operation. It holds the
initial native file references during the asynchronous reads and rechecks
all surviving references together at the end. The original form association
and current document must still agree. The final report records each verified
digest and size or the reason for review. The review panel reports how many
attachments match and identifies the field when a selection changed or was
lost.

## Qualification

The headless fixtures use two distinct bundled fictional PDFs in successive
sections. The runtime retained the first file input across the section
change. All variants used the same prepared names and independent digests.

| Terminal state | Observed result |
| --- | --- |
| Both original selections retained | Both PDF digests matched |
| Both original inputs without a native form owner | Both PDF digests matched |
| Cover letter below the overview image | Both PDF digests matched after regional review |
| Résumé content replaced with the same filename and size | Résumé pending and cover letter verified |
| Résumé input detached | Résumé pending and cover letter verified |
| New input with the same selection replaces the original | Résumé pending and cover letter verified |
| Original input associated with a different form | Résumé pending and cover letter verified |
| Original selection cleared | Résumé pending and cover letter verified |
| Actual local vision on the complete two-PDF summary | Both filenames and both PDF digests matched |

The non-model variants used prescribed image observations to isolate native
behavior. The actual local visual case used Qwen 3.8 27B Q3_K_XL in private
Ollama 0.40.16 with a 16,384-token context. Its entry and navigation checks
used deterministic test callbacks. The final image reading was independent
of the prepared answers. The run took 56.288 seconds and had a measured
minimum available host RAM of 12.284 GiB.

An independent script exercised the readback function with the earlier
selection changing during the later file digest. It refused the earlier file
and retained the later one. Two existing headless browser tests also passed
for declared file formats and multiple selections in one input.

All nine replays closed their browsers and recorded zero final-review scroll
events, cached write responses or external deliveries. Private native
receipts, fixtures and rendered UI evidence are under
`target/terminal-attachments-ad350d39/`.

After restoring the regular-file check before opening a prepared path,
the retained and changed-byte cases passed again on the final test
executable. The final private Qwen replay again verified both PDFs. It ran
for 64.998 seconds and kept at least 11.677 GiB of available host RAM in
the samples. The final run records are under
`target/terminal-attachments-ad350d39/final/`.

The final workspace run passed 1,475 tests with 621 ignored.
Warnings-denied Clippy, the Windows build, two headless browser attachment
regressions and all 172 UI scripts passed. The saved mismatch panel fit at
430 and 1,024 pixels and under 200% CSS zoom. The narrow render was inspected
after the comparison list replaced the general review message.

## Coverage

This establishes the bytes that remained available in the original local
browser selection. A detached filename, a disabled or changed input and a
server-managed upload without current local bytes require review. Broader
held-out workflows and external acceptance remain separate qualification
work.
