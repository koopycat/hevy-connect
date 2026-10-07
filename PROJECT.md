---
status: experiment
created: 2026-10-07
review_after: 2027-01-05
---

# hevy-connect

## Intent

Determine whether Hevy's early Public API is sufficient for safe, agent-ergonomic access to training data and supported writes. Capture the full documented contract, compare it with live behavior, and implement a TypeScript AXI CLI without implying access to undocumented AI Trainer plans.

## Outcome

- Completed a deep analysis of the official OpenAPI contract: 14 paths, 22 operations, authentication, pagination, mutation semantics, inconsistencies, and API risks are recorded in `docs/hevy-api-analysis.md`.
- Built `hevy-axi`, a full TypeScript CLI for all 22 operations, with compact TOON output, JSON and field projection, bounded pagination, structured errors, secure credential resolution, explicit mutation confirmation and dry runs, and optional ambient agent hooks.
- Safety behavior includes no POST/PUT retries, full-state handling for replacement writes, read-merge-write measurement updates, credential redaction, and no response or health-data cache.
- Validation is comprehensive and currently passes `just check`; use that canonical command rather than relying on a fixed test count.
- Observed live deviations are handled defensively: workout events requested without `since` can arrive under a `workouts` envelope, and routine folders can arrive under the legacy `routines` envelope instead of documented keys.
- The Public API does not document an operation for Hevy's AI Trainer; its plans must not be presented as available through this CLI.

## Decision

- [ ] Keep or promote
- [ ] Trash after review
- [ ] Keep as reference
