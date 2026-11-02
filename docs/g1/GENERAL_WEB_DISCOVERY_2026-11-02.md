# General web discovery and listing inspection — 2026-11-02

An isolated workspace exercised the application's web-search batch API against FreeSerp using two non-personal phrases: `materials scientist jobs Zurich` and `backend Rust engineer remote jobs`. Two requested pages succeeded and returned ten unverified candidates each. The provider did not report engine availability. The results included job-board search pages and other non-vacancy pages, so they must not be counted as twenty confirmed jobs.

One candidate returned HTTP 404 when independently read with the repository's isolated Crawl4AI provider; the application also rejected it with an HTTP 404 review message. A separate Greenhouse candidate returned HTTP 200 and contained the expected job title in a Crawl4AI capture. Its embedded form exposed custom controls and a CAPTCHA to the browser. Earlier application inspection stopped before extracting the listing; the updated inspection retained those warnings, used a resource-gated local `qwen3.5:9b` model to extract a source-checked title and employer, and saved the original page capture in the isolated workspace. The model did not return a verifiable description excerpt, so the saved description remains incomplete and the application flagged manual review, including a separate CAPTCHA reason. The capture is not proof that the job remains open or that its application is ready.

The 404 and 200 native research captures were retained outside Git with SHA-256 digests `889bda9bd4de6a416e39c77b754fb34e17538ac9e2cb555f35400a1f0c74c618` and `53ba81f744531e7741b1901205545f6d2572f94de0773caab24cd39a0b31ff97`, respectively. These checks used application APIs, not an end-to-end interface test. No applicant data was entered, no CAPTCHA was solved, and no application was submitted. The local model was unloaded after testing.

## Source-bound description fallback — 2026-11-02

A bounded local fallback now divides captured listing text into verbatim segments and asks the model to select one relevant to the identified role when the model's copied description is absent or invalid. It rejects invalid indexes, retains the exact source text, and labels the result as a model-selected partial excerpt requiring review. It runs only when the separate page classifier reports a vacancy and no narrower selected section exists; this does not prove the selected segment actually belongs to the role.

On the previously saved public Greenhouse capture, the resource-gated `qwen3.5:9b` test recovered a 497-byte source-matched excerpt containing role responsibilities after the original extraction returned no verifiable description. The test used the already captured page and did not make another employer request. The result is one example, not a general accuracy measurement. It did not change the saved job or submit an application; the model was unloaded.
The fallback inspects at most 32 segments and reports when that limit leaves source text unexamined. Segment boundaries prefer sentence ends, then whitespace; very long unbroken text may still be split. A selected excerpt is not a full job description.

## Longer source-bound excerpts — 2026-11-02

The fallback can now select a single contiguous range of up to 12 indexed segments and 6,000 bytes, preserving the exact source text between the chosen boundaries. Invalid, reversed, incomplete and oversized selections are rejected. The original one-index response remains accepted as a single segment. Results are still partial model-selected excerpts and require review against the original listing.

On the same saved public listing, an initial four-segment limit rejected the model's nine-segment selection. With the revised bounds, one resource-gated 9B run recovered a 4,003-byte exact source excerpt. This was an offline test against the earlier capture, not a new application or a measurement of general extraction accuracy. The model was unloaded afterward.

### Role association check (2026-11-02)

A resource-gated local `qwen3.5:9b` evaluation used three fictional multi-role page layouts. The initial role identity and vacancy classification were seeded, so this isolates description selection rather than measuring end-to-end discovery. Before the change, a reversed layout yielded an excerpt combining duties from different roles, and role enumeration did not reliably catch it. With an additional model association review, one layout retained a 927-byte excerpt and two were withheld for review. An unbounded explicitly selected role section is also withheld. A previously captured public single-vacancy listing retained a 4,003-byte source-bound excerpt; no new employer request was made.

The association review uses the same local model and is not independent proof of correctness. Retained excerpts still require review; these examples do not establish general accuracy across sites.
