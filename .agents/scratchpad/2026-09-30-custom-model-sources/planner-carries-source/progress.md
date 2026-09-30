# Progress — planner-carries-source
- [x] `PackDir.source`, `Plan.entries` / `labels()` / `source_counts()`, `SourceResolver`, sourced notes ("in source X")
- [x] Existing tests migrated (207 total after 6 new), harness green
- [x] `//!` layout contract rewritten
## Cycles
1. Field/API change + test migration → green (behaviour preserved). 2. Six source scenarios → green first run (the resolver was written against them).
## Deviations
- Added `Plan::source_counts()` (the scanner's INFO needs it) — small helper, no interface change elsewhere.
Status: Complete (uncommitted — maintainer commits manually)
