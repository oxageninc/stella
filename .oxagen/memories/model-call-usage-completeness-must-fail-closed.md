---
name: model-call-usage-completeness-must-fail-closed
type: bug
domain: telemetry
severity: P1
linear: none
date: 2026-07-21
---

**Symptom:** Direct pipeline model calls emitted only aggregate budget ticks, multi-turn engine calls reused telemetry identities, and persistence failures could still produce export-eligible execution rollups.
**Root cause:** Paid-call accounting was split across engine, pipeline, renderer, and store boundaries without a durable monotonic completeness invariant.
**Fix:** Emit one role/provider-attributed call envelope at the no-await settlement boundary, emit content-free incompleteness on unknown failures, use event sequence as execution-global call identity, and carry persistence completeness monotonically into execution closeout and export eligibility.
**Guard:** Focused protocol, engine cancellation/failure, pipeline role matrix, CLI persistence, migration, and bounded-backfill tests fail on the old behavior and pass on the corrected path.
**Watch-outs:** Any new `Provider::complete` caller must share the metered call chokepoint or prove that the provider is non-billable; aggregate budget totals cannot reconstruct missing per-call telemetry.
