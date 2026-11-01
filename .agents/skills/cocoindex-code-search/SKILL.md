---
name: cocoindex-code-search
description: Route broad repository searches through a local source-bound index, then verify the live source.
---

# CocoIndex Code search

Before broad or conceptual repository navigation, use the local adapter:

```powershell
py -3.13 -B tools/cocoindex_agent_search.py install
py -3.13 -B tools/cocoindex_agent_search.py index
py -3.13 -B tools/cocoindex_agent_search.py search --limit 5 "question"
```

Installation and indexing are explicit and can download a model. Indexing uses
tracked source bytes, including edits, and keeps its private mirror and receipts
outside the checkout. It rejects stale indexes, concurrent operations and
secret-like tracked paths. Search only with non-sensitive queries. A receipt
records the source digest, query digest and actual result; confirm every candidate
with exact `rg` and direct reads. Use `rg` directly for exact names and small
known scopes. Never overwrite another repository's registration or cache.
Diagnose and disclose failures; continue only independent work until restored.
The installer pins the direct CocoIndex version; its transitive dependencies are
not hash-locked, so do not claim a verified reproducible installation.
