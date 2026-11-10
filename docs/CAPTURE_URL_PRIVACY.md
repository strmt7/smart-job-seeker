# Anonymous capture URL checks

Checkpoint: 2026-11-10. The source-copy parent previously screened its entry URL,
while redirects and browser-discovered references did not receive the same
privacy checks. A shared Rust check now runs before those resource reads,
including robots redirects and the collector's transport boundary. Source
permission, HTTPS/DNS validation, robots rules and request budgets remain
separate requirements; this check cannot authorize a source or candidate entry.

## Behavior and limits

Known authentication/contact query keys, including separator/bracket variants,
email-like references and user information are refused. The checker examines
nested HTTP(S) references, network/absolute-path/query references and up to three
percent-decoding layers. Scheme matching is case-insensitive. Malformed
UTF-8/escapes, control characters, excessive nesting/encoding, more than 64 query
pairs or references longer than 4,096 bytes require review. The original URL is
used for permitted requests; decoded text is only for local checks.

The app uses that decoded view for its existing known-candidate identifier
comparison. This catches repeatedly encoded names at the initial handoff
without sending profile data to the capture worker. Public job IDs, language
parameters and scoped asset paths such as `@scope/package@1.2.3` remain usable.
No new dependency or model call is needed.

Refusals produce explicit `private_url` or `url_privacy_uncertain` outcomes;
malformed input remains `invalid_url`. Reports retain the existing URL hash,
not a refused URL/value. A discovered refusal keeps the source copy in review
even if the other resources form a valid archive. An incomplete entry cannot
produce a usable final archive.

This is a bounded heuristic, not a general candidate-data provenance proof.
Opaque identifiers, arbitrary encodings, server-returned personal content and
unknown private parameter names remain unclassified. Candidate-specific checks
occur in the parent; the anonymous worker receives no profile and cannot compare
every resource with the candidate's facts. Benign parameters named like private
ones can also need manual review. These checks must never be treated as proof
that a live form is safe to interact with.

## Measured connected workflow

A fictional HTTP source supplies an entry redirect, JavaScript, styles and
questions. The real contained browser discovers public assets plus a token
reference, an email reference and a public route redirecting to a private
session URL. All three private targets are refused before collection transport
reads. The public redirect route itself is read to observe its Location.

Eight anonymous fixture reads recorded six resources through three discovery
rounds. All three refusals remained visible as review outcomes. After the
collector was consumed, a separate contained browser verified Unicode name
entry, opened the next section and verified the fictional email. It served five
archived replies and refused three unrecorded requests, with no collector reuse.
Commit was refused and the browser closed. The final screenshot is byte-identical
to the directly inspected capture from the earlier lowercase-reference case.
No public site, real form, real candidate, application email or submission was used.

Private source/engine-bound evidence is in
`target/capture-replay-qualification-u3z1jen8/result.private.json`.
The final case uses an uppercase scheme inside an encoded nested token reference.
One opt-in native test executed with zero ignored cases in 35.888 seconds.
Seventy-two whole-host samples, nominally 500 ms apart, recorded at least
25,512,878,080 bytes (23.761 GiB) available RAM. These are host observations,
not a per-application peak, reservation or long-running leak qualification.
The native result SHA-256 is
`1f5f1cc75faf560d52d92948d48a523e29440636d469939f422e869572849df1`;
the final screenshot SHA-256 is
`0aab9440c4178651125f84e8e161bb435d755d15242eb06a00dd8ea6e036250a`.

Reproduce headlessly using a verified private headless-shell directory:

```powershell
py -3.13 -X utf8 -B tools/qualify_capture_replay.py --engine <absolute-headless-shell-directory> --privacy
```

Seven new ordinary checks cover private keys/encodings, public-reference
preservation, malformed input, query/nesting budgets and direct/redirect/robots/
discovery refusals with transport-call assertions. The ordinary workspace passed
1,218 tests with 579 opt-in cases ignored across
43 reported targets. Workspace Clippy, formatting and the managed app build
passed. Unchanged interface, local-model
and broad browser matrices were not repeated for this URL-boundary change.
Remaining [isolation/provenance gates](ISOLATED_REHEARSAL.md) are still open;
the interface does not enable captured real-site candidate entry.
