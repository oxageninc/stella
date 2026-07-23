# ADR 0001: Semantic Taxonomy

- Status: Accepted (Phase 0)
- Date: 2026-07-23
- Deciders: (Phase 0 baseline)

## Context

The adaptive-context work needs one typed record taxonomy that every later phase
(migration, compilation, learning, governance) builds on. Two incompatible
taxonomies exist in the planning corpus. The canonical pair
(`adaptive-context-implementation-plan.md` §5 and `stella-adaptive-context-lifecycle.md`)
defines separate record families; the older `directive-schema.md` (Jul 20)
defines a single flat `kind` list. These cannot both be implemented — they
assign different meanings to the same words.

This ADR RECORDS the taxonomy decision already made by the canonical pair. It
does not open a new question.

## Decision

Adopt the plan's taxonomy: separate `knowledge`, `memory`, and `directive`
families, typed as `ContextRecordKind` with per-family `KnowledgeKind`,
`MemoryKind`, and `DirectiveKind`. Confidence is an integer `0..=100`
(lifecycle §7.6). `constraint_effect` is `require` or `forbid` — **never**
`allow` (plan §5, lifecycle §11 "forbid").

The older `directive-schema.md` taxonomy — `kind: memory|fact|rule|preference|
constraint|procedure`, confidence `0–1`, `priority low|normal|high|critical` —
is **SUPERSEDED and must not be implemented**. Per the roadmap §2, `memory` and
`fact` must not be restored as directive kinds. Mine that doc for lifecycle
*ideas* only (citation stats, archive ratios), never its type shapes.

The existing `stella-core/src/rules/metadata.rs` `RuleRecordKind::Directive`
(carrying `scope_paths`/`enforcement`/`confidence`) is subsumed and renamed
into this taxonomy in Phase 1; it is not deleted in Phase 0.

## Consequences

Phase 1 introduces these as pure I/O-free types in `stella-core`. Migration
(Phase 2) maps ambiguous legacy memories losslessly as `memory`, never
reclassifying by LLM. Any code or doc reaching for the flat `directive-schema`
kinds is a regression to reject in review.

## Open questions

Related open item (do not resolve here): `Origin` arity disagrees inside the
canonical pair — §7.1 lists five (`user, system, observed, inferred, imported`)
but the §8.6 directive example permits four (drops `observed`). Likely a
deliberate per-kind narrowing; verify before the enum freezes in Phase 1.
