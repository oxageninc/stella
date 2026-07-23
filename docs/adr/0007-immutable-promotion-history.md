# ADR 0007: Immutable Promotion History

- Status: Proposed — needs human confirmation
- Date: 2026-07-23
- Deciders: (Phase 0 baseline)

## Context

The adaptive loop promotes observations into proposals into governed records.
For that loop to be auditable and reversible, the promotion trail cannot be
editable after the fact. The canonical plan makes promotions immutable, driven
by a governance state machine. This ADR RECORDS that decision — and FLAGS one
surface conflict between the canonical plan and `context-prs-spec.md` that must
be resolved before Phase 6, because it fixes an enforcement enum.

## Decision

Promotions (`promotion_event`) are **append-only and immutable**. A governance
state machine with modes `solo`, `team`, and `regulated` records every
transition; state changes create new immutable events (consistent with ADR
0004), never in-place edits.

**Enforcement-level mapping (proposed, not ratified):** `context-prs-spec.md`
uses four levels (`observe | advisory | required | blocking`); the canonical
plan uses two (`advisory | blocking`). Record the roadmap's proposed 4→2
mapping — `observe`/`advisory` → `advisory`; `required`/`blocking` → `blocking`
— while leaving open the alternative of keeping four as a UI ramp over two
enforcement states. Also frame "Context PR" as UX over the
`record_proposal → promotion_event` pipeline, not a second mechanism.

This mapping requires human sign-off before Phase 6.

## Consequences

Phase 6 emits immutable `PromotionRecorded` events with a re-proposal cooldown;
no inferred directive may auto-activate as blocking, and no sharing widens
automatically (M-B gate). Whichever enforcement resolution is ratified fixes the
`DirectiveEnforcement` value set and the proposal-review UX, so it must be
locked first.

## Open questions

**Needs human confirmation:** ratify the 4→2 enforcement mapping (vs. retaining
four levels as a UI ramp) before Phase 6, since it freezes the
`DirectiveEnforcement` enum.
