# Architecture Decision Records — Phase 0 (Adaptive Context)

These ADRs capture the baseline decisions for Phase 0 of the adaptive-context
work in stella. Most RECORD decisions already made by the canonical planning
pair (`adaptive-context-implementation-plan.md` and
`stella-adaptive-context-lifecycle.md`); two FLAG open questions that require
human sign-off before the enums they touch are frozen in later phases, and must
not be resolved by fiat. Each ADR grounds its claims in the source docs and, where
relevant, the current stella code. The two marked below carry open questions
requiring human confirmation before their gating phase.

| # | Title | Status |
|---|---|---|
| [0001](0001-semantic-taxonomy.md) | Semantic Taxonomy | Accepted (Phase 0) |
| [0002](0002-scope-vs-sharing.md) | Scope vs. Sharing | **Proposed — needs human confirmation** (SharingScope arity, before Phase 1) |
| [0003](0003-bitemporal-semantics.md) | Bitemporal Semantics | Accepted (Phase 0) |
| [0004](0004-record-revision-identity.md) | Record Revision Identity | Accepted (Phase 0) |
| [0005](0005-storage-authority.md) | Storage Authority | Accepted (Phase 0) |
| [0006](0006-contextframe-vs-compiledcontextframe.md) | ContextFrame vs. CompiledContextFrame | Accepted (Phase 0) |
| [0007](0007-immutable-promotion-history.md) | Immutable Promotion History | **Proposed — needs human confirmation** (enforcement 4→2 mapping, before Phase 6) |
| [0008](0008-markdown-canonical-rules.md) | Markdown Repository Rules Remain Canonical | Accepted (Phase 0) |
