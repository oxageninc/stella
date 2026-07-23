# ADR 0002: Scope vs. Sharing

- Status: Proposed — needs human confirmation
- Date: 2026-07-23
- Deciders: (Phase 0 baseline)

## Context

Two orthogonal dimensions are easy to conflate and must stay separate. `Scope`
answers *where a record applies* (which repository, workspace, user, or
organization it is about). `SharingScope` answers *who may receive or inherit
it* (lifecycle §7.3). Authority is derived from origin and evidence, not from
sharing rank (§13.2) — so the two cannot be collapsed into a single ladder.

This ADR FLAGS an open decision. The canonical pair disagrees with itself on
`SharingScope` arity, and this value set becomes a `context_records.sharing_scope`
column the Phase 1/2 implementer hits immediately, so it must be resolved
before the enum is frozen.

## Decision

Keep `Scope` and `SharingScope` as distinct types (plan §5).

For `SharingScope` arity, record the roadmap's **provisional** resolution: treat
the **four-value** set — `user`, `repository`, `workspace`, `organization`
(lifecycle §7.3) — as authoritative. `workspace` is first-class in §13.4
workspace publication, and the §21 acceptance line ("SharingScope contains only
user, repository, and organization") is judged stale. Repository and workspace
are not synonyms: a repository is a VCS/Git identity; a workspace is a
provider-managed security principal spanning resources (§7.3).

This resolution is **not** ratified. It requires human sign-off before the enum
is frozen in Phase 1.

## Consequences

Records carry both a `scope_json` and a `sharing_scope`. Audience changes are
always explicit — sharing never widens automatically (roadmap M-B gate). If a
reviewer instead confirms the three-value set, the `workspace` publication path
(§13.4) must be re-scoped and the column value set narrowed.

## Open questions

**Needs human confirmation:** ratify the 4-value `SharingScope` (accepting §21
as stale) versus the 3-value set, before the enum is frozen in Phase 1.
