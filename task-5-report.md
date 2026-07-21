# Task 5 report: isolated, typed witness execution

## Outcome

Task 5 is complete. Model- and user-authored test text now crosses a strict
parser into `TestInvocation { program, args }` and runs through a direct
process `TestRunner`, never through `bash -c`. An authored witness is created,
baselined, exercised, revised, and finally verified inside the same disposable
candidate workspace, including when only one candidate is requested.

Only a candidate with a passing final verdict may be adopted. Failed,
aborted, or red candidates are removed, and witness-isolation failure aborts
before witness authoring can touch the session tree.

## TDD evidence

RED was established before implementation:

- The parser tests failed to compile because `TestInvocation` and
  `parse_test_invocation` did not exist. They then proved shell operators,
  redirection, expansion, unknown programs, and malformed quoting are rejected.
- Witness-artifact tests failed to compile before
  `validate_witness_artifact` existed. They now prove tracked edits,
  pre-existing untracked edits, non-test files, and multi-file authoring are
  rejected.
- A same-length edit with restored mtime passed the legacy `len:mtime`
  fingerprint. It became detectable only after fingerprints changed to
  SHA-256 over the complete file bytes.
- A direct-runner test initially had no typed execution boundary. It now
  proves a redirection token remains a literal argv item and creates no file.
- The one-candidate witness regression initially touched the session
  `RepoStatusPort` and panicked. It now authors and verifies entirely inside
  one disposable candidate.
- The isolation-failure regression initially reached session state. It now
  aborts before emitting the witness stage.
- The final-red regression initially logged winner adoption. It now logs no
  adoption and removes every candidate.
- The tracked-production-edit regression initially allowed witness author
  contamination. It now aborts the candidate, removes it, and never adopts.
- Task 4's post-witness routing-cost regression caught an ordering change; the
  worker is still resolved after paid witness authoring so settled cost is
  retained on routing failure.

## Implementation notes

- `TestInvocation` and `TestRunner` form a typed, shell-free test boundary.
  The CLI adapter launches the known program with its explicit argv and a
  workspace root. `ShellCommandRunner` remains separate for fixed diagnostic
  diff probes.
- Configured test commands are parsed before the first paid pipeline stage.
  Witness commands are parsed immediately after authoring or repair.
- The accepted command vocabulary is intentionally narrow: Cargo test and
  nextest, common JavaScript package test runners, pytest, Go test, and .NET
  test.
- Witness validation compares complete tracked and untracked before/after
  maps and accepts exactly one newly created test-shaped artifact.
- Both session and candidate repo-status adapters hash complete bytes. Tracked
  deletions receive a sentinel so they remain visible.
- Every authored witness gets a candidate-local authoring pass. Its baseline,
  worker execution, revision loop, tamper checks, and final verification reuse
  that candidate's tools, status ports, and typed test runner.
- Adoption is gated on `verdict.passed`. Every other workspace is removed;
  the existing recovery exception for a passing winner whose adoption itself
  conflicts remains intact.
- Task-specific production and test modules keep the existing pipeline files
  below their size ratchets.

## Verification

- `cargo test -p stella-pipeline`: 123 unit tests and 4 replay tests passed.
- `cargo test -p stella-cli`: 329 tests passed, including all 10 candidate
  workspace tests and the typed-runner/fingerprint regressions.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `scripts/check-file-sizes.sh`: passed for all 293 tracked Rust files.
- `git diff --check`: passed.

## Self-review

- Searched all pipeline command execution sites and confirmed test observations
  use `TestRunner`; only diagnostic diff commands retain the shell runner.
- Reviewed every candidate exit path. Isolation failures, witness failures,
  worker-routing failures, red verification, and non-winning candidates all
  clean up without adoption.
- Corrected stale witness documentation that still described a permissive
  multi-file watchlist; the accepted artifact is now exactly one new test file.
- Re-ran the full pipeline and CLI suites after the module extraction and
  Clippy fixes.

## Concerns

No known correctness concerns remain in Task 5 scope. No push was performed.
