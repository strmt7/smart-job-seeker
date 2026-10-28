---
name: waypoint-job-discovery
description: Research and implement broad, source-aware job discovery and application-page verification in Waypoint.
---

# Waypoint job discovery

Use this workflow when changing job discovery, source adapters, application-link checks, or evidence for those features. It adapts the `web-discovery` workflow catalogued in the user's AI Skills Collection Benchmarked at commit `4bd209752567f5c35074d4ee4a73600e1c45c283`; its upstream source is [project_air_defense's web-discovery skill at commit 77f082fe23a86ded15e696a22955b2a890da2fce](https://github.com/strmt7/project_air_defense/blob/77f082fe23a86ded15e696a22955b2a890da2fce/.agents/skills/web-discovery/SKILL.md). The catalog entry records no validated runtime benchmark; do not claim one.

1. Identify whether the task needs source research, search-provider integration, job parsing, duplicate review, or application-page verification. Use current repository evidence and the required source-bound search tooling before broad code navigation.
2. For public research, follow the installed Crawl4AI research skill. Prefer official API documentation and employer pages, preserve dated extraction receipts, and verify a source's permissions, rate limits and attribution requirements before integration. Search terms sent externally must not contain candidate details unless the user deliberately supplies or approves them.
3. Support varied user or local-model search phrases and multiple sources. Treat provider limits, pagination, network budgets and incomplete coverage as visible outcomes; do not imply that a fixed set of boards covers the global job market.
4. Keep each listing's original source URL and available description. Distinguish a job listing from an actual application page, and classify blocked, ambiguous, sign-in or CAPTCHA flows for manual review. Do not bypass challenges.
5. Treat possible duplicates as reviewable evidence. Similar titles, employer aliases, locations and descriptions can indicate the same role, but a fuzzy score alone must not silently merge distinct vacancies or authorize a submission.
6. Test with fictional local fixtures first and state exactly which providers, forms and models were actually exercised. No application or email may be sent without the user's explicit authorization.

Use the repository's current instructions and the user's requested scope over any source workflow. Keep provenance and licensing with any imported code or assets.
