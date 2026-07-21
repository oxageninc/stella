//! Witness authoring: the front half of deterministic verification (L-E11).
//! When no `--test-command` is configured, the pipeline asks an independent
//! model (the judge's resolution — witness ≠ worker, so the test that defines
//! "done" is authored by the same independent role that enforces it) to write
//! a **witness test**: a test that FAILS on the current code and will pass
//! once the goal is met. Its command becomes the flip oracle's tracked
//! command, so the repo's defining contract — "verified done, not claimed
//! done" — holds even when the user armed nothing.
//!
//! # Visible, not hidden — integrity by tamper exclusion
//!
//! The witness is deliberately **visible to the worker**: iterating against a
//! failing test is where convergence comes from, and a test file on disk is
//! discoverable by any worker with a shell anyway. Integrity comes instead
//! from *tamper exclusion* — complete content hashes of the one test artifact
//! the witness turn created are snapshotted, and a
//! flip is only credited if those fingerprints are unchanged at verify
//! time ([`tampered_paths`]). A worker that edits or
//! deletes the witness loses the deterministic flip credit and the evidence
//! reaching the judge names the tampered paths. This mirrors how SWE-bench
//! itself scores (the scored test patch is applied outside the worker's
//! diff), at a fraction of the machinery of actually hiding a file.
//!
//! # The pure/orchestration split
//!
//! Like `triage`/`verify`, everything here is a synchronous function over
//! owned data: prompt builders, the response parser, the watchlist delta, and
//! the tamper check. Running the witness engine turn, executing the authored
//! command, and the one bounded repair retry live in [`crate::pipeline`].

use std::collections::HashMap;

use crate::ports::{RecalledFrame, TestInvocation};

/// The marker line the witness author must end its reply with. Scanned
/// case-insensitively by [`parse_witness_command`]; the LAST occurrence wins
/// (the model may quote the marker while reasoning before its final answer).
pub const TEST_COMMAND_MARKER: &str = "TEST_COMMAND:";

/// Why a model- or user-authored test command was not accepted as a typed
/// test invocation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TestInvocationError {
    /// The command was empty or contained unbalanced quoting.
    #[error("the test command is empty or has invalid quoting")]
    InvalidSyntax,
    /// Shell control syntax is never valid at this boundary.
    #[error("shell operators, redirection, and expansion are not allowed in test commands")]
    ShellSyntax,
    /// Only explicit test-runner forms are accepted.
    #[error("unsupported test command `{0}`")]
    Unsupported(String),
}

/// A witness author crossed the one-new-test-file boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WitnessArtifactError {
    /// Existing tracked content was changed or removed.
    #[error("witness author modified tracked file(s): {}", .0.join(", "))]
    TrackedMutation(Vec<String>),
    /// The untracked delta was not exactly one newly created test artifact.
    #[error("witness author must create exactly one new test file; changed: {}", .0.join(", "))]
    InvalidArtifact(Vec<String>),
}

/// Parse a deliberately small test-command vocabulary into an enumerable
/// program plus argv. This is quote-aware only to preserve arguments with
/// spaces; it is not a shell parser and rejects every shell control surface.
pub fn parse_test_invocation(command: &str) -> Result<TestInvocation, TestInvocationError> {
    let words = split_test_words(command)?;
    let (program, args) = words
        .split_first()
        .ok_or(TestInvocationError::InvalidSyntax)?;
    let allowed = match program.as_str() {
        "cargo" => {
            matches!(args.first().map(String::as_str), Some("test"))
                || matches!(
                    (
                        args.first().map(String::as_str),
                        args.get(1).map(String::as_str)
                    ),
                    (Some("nextest"), Some("run"))
                )
        }
        "pnpm" | "npm" | "yarn" | "bun" => {
            matches!(args.first().map(String::as_str), Some("test"))
        }
        "pytest" => true,
        "python" | "python3" => matches!(
            (
                args.first().map(String::as_str),
                args.get(1).map(String::as_str)
            ),
            (Some("-m"), Some("pytest"))
        ),
        "go" | "dotnet" => matches!(args.first().map(String::as_str), Some("test")),
        _ => false,
    };
    if !allowed {
        return Err(TestInvocationError::Unsupported(command.to_string()));
    }
    Ok(TestInvocation {
        program: program.clone(),
        args: args.to_vec(),
    })
}

fn split_test_words(command: &str) -> Result<Vec<String>, TestInvocationError> {
    if command.contains("$(") || command.contains('`') || command.contains('\n') {
        return Err(TestInvocationError::ShellSyntax);
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut single = false;
    let mut double = false;
    let mut chars = command.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !double => {
                single = !single;
                started = true;
            }
            '"' if !single => {
                double = !double;
                started = true;
            }
            '\\' if !single => {
                let Some(escaped) = chars.next() else {
                    return Err(TestInvocationError::InvalidSyntax);
                };
                current.push(escaped);
                started = true;
            }
            '&' | '|' | ';' | '<' | '>' if !single && !double => {
                return Err(TestInvocationError::ShellSyntax);
            }
            c if c.is_whitespace() && !single && !double => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if single || double {
        return Err(TestInvocationError::InvalidSyntax);
    }
    if started {
        words.push(current);
    }
    Ok(words)
}

/// Validate the witness author's complete working-tree delta and return the
/// content-hash baseline for the one accepted test artifact.
pub fn validate_witness_artifact(
    tracked_before: &HashMap<String, String>,
    tracked_after: &HashMap<String, String>,
    untracked_before: &HashMap<String, String>,
    untracked_after: &HashMap<String, String>,
) -> Result<HashMap<String, String>, WitnessArtifactError> {
    let tracked = changed_paths(tracked_before, tracked_after);
    if !tracked.is_empty() {
        return Err(WitnessArtifactError::TrackedMutation(tracked));
    }
    let changed = changed_paths(untracked_before, untracked_after);
    let accepted = match changed.as_slice() {
        [path]
            if !untracked_before.contains_key(path)
                && untracked_after.contains_key(path)
                && is_test_path(path) =>
        {
            path
        }
        _ => return Err(WitnessArtifactError::InvalidArtifact(changed)),
    };
    Ok(HashMap::from([(
        accepted.clone(),
        untracked_after[accepted].clone(),
    )]))
}

fn changed_paths(before: &HashMap<String, String>, after: &HashMap<String, String>) -> Vec<String> {
    let mut paths: Vec<String> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    normalized
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__" | "spec" | "specs"))
        || name.starts_with("test_")
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("witness")
}

/// A validated witness: the flip-oracle command plus the fingerprint
/// hash of the one new test artifact the witness turn created (the tamper
/// baseline for [`tampered_paths`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witness {
    /// The user-facing command the flip oracle names in evidence.
    pub command: String,
    /// Parsed process invocation used for every baseline/final test run.
    pub invocation: TestInvocation,
    /// `path -> fingerprint` for the one accepted, newly created test file.
    /// Tracked edits, non-test files, and edits to pre-existing untracked
    /// files are rejected before candidate execution.
    pub files: HashMap<String, String>,
}

/// The witness author's task prompt: split context exactly like the planner
/// (goal + recall + repo structure, never the worker transcript — L-E6). The
/// hard requirements — new file only, must fail now, no production edits,
/// marker line — are the parts [`parse_witness_command`] and the pipeline's
/// fail-check enforce mechanically; the prose is guidance.
pub fn witness_prompt(goal: &str, recall: &[RecalledFrame], repo_structure: &str) -> String {
    let mut s = String::from(
        "You are the WITNESS AUTHOR for a coding agent. Write a witness test: a minimal \
         test that FAILS on the current code and will PASS once the goal below is correctly \
         accomplished. The fail→pass flip of your test is what verifies the work.\n\n\
         Hard requirements:\n\
         - Create ONE NEW test file. Never modify existing files, and never touch \
         production code — the implementation is someone else's job.\n\
         - The test must fail NOW for the RIGHT reason (it exercises the missing/broken \
         behavior), not because of a typo, a missing import, or a harness error.\n\
         - Prefer the narrowest runnable command (one test/module, not the whole suite).\n\
         - End your reply with exactly one line:\n\
         TEST_COMMAND: <a direct test command such as cargo test or pytest>\n",
    );
    if !repo_structure.trim().is_empty() {
        s.push_str("\n## Repository structure\n");
        s.push_str(repo_structure.trim());
        s.push('\n');
    }
    if !recall.is_empty() {
        s.push_str("\n## Recalled context\n");
        for f in recall {
            s.push_str("- [");
            s.push_str(&f.citation_label);
            s.push_str("] ");
            s.push_str(f.content.trim());
            s.push('\n');
        }
    }
    s.push_str("\n## Goal\n");
    s.push_str(goal.trim());
    s
}

/// The one bounded repair retry (the L-V2 pattern): the authored test passed
/// on the *unmodified* code, so it witnesses nothing. Sent into the same
/// witness thread; a second failure to produce a failing test discards the
/// witness (the pipeline degrades to judge-based verification, never loops).
pub fn witness_repair_prompt(command: &str) -> String {
    format!(
        "Your witness test PASSED on the current, unmodified code — it proves nothing, \
         because only a fail→pass flip counts as verification. Rewrite the test so it fails \
         NOW for the right reason (it must exercise the behavior the goal will add or fix). \
         The command that just passed was:\n{command}\n\n\
         End your reply with the corrected `TEST_COMMAND:` line."
    )
}

/// Extract the witness command from the author's reply: the LAST
/// `TEST_COMMAND:` line (case-insensitive), stripped of surrounding
/// whitespace and backticks. `None` when no non-empty command is found — the
/// caller treats that like a failed witness stage (degrade, never guess).
pub fn parse_witness_command(text: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('`');
        if trimmed.len() >= TEST_COMMAND_MARKER.len()
            && trimmed[..TEST_COMMAND_MARKER.len()].eq_ignore_ascii_case(TEST_COMMAND_MARKER)
        {
            let cmd = trimmed[TEST_COMMAND_MARKER.len()..]
                .trim()
                .trim_matches('`')
                .trim();
            if !cmd.is_empty() {
                found = Some(cmd.to_string());
            }
        }
    }
    found
}

/// The witness watchlist: every untracked file the witness turn created or
/// modified, as `path -> fingerprint` — present in `after` with no `before`
/// entry or a different fingerprint. This *observed* delta is the tamper
/// baseline; the author's own claims about which files it wrote are never
/// trusted (a wrong claim would corrupt tamper detection, an observed delta
/// cannot).
pub fn witness_watchlist(
    before: &HashMap<String, String>,
    after: &HashMap<String, String>,
) -> HashMap<String, String> {
    after
        .iter()
        .filter(|(path, fp)| before.get(*path) != Some(*fp))
        .map(|(path, fp)| (path.clone(), fp.clone()))
        .collect()
}

/// Tamper check: which watchlisted witness files are no longer byte-identical
/// (fingerprint changed) or gone (deleted / moved out of the untracked set)
/// at verify time. Non-empty means the deterministic flip must NOT be
/// credited — the evidence degrades to inconclusive and the judge is told
/// which paths were touched. Sorted for deterministic evidence text.
pub fn tampered_paths(
    watchlist: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> Vec<String> {
    let mut tampered: Vec<String> = watchlist
        .iter()
        .filter(|(path, fp)| current.get(*path) != Some(*fp))
        .map(|(path, _)| path.clone())
        .collect();
    tampered.sort();
    tampered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fps(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(p, f)| (p.to_string(), f.to_string()))
            .collect()
    }

    // ---- parse_witness_command -------------------------------------------

    #[test]
    fn parses_a_bare_marker_line() {
        assert_eq!(
            parse_witness_command("done.\nTEST_COMMAND: cargo test -p x witness_"),
            Some("cargo test -p x witness_".to_string())
        );
    }

    #[test]
    fn last_marker_wins_and_backticks_are_stripped() {
        let text = "I will end with `TEST_COMMAND: placeholder`\n\
                    ...work...\n\
                    TEST_COMMAND: `pytest tests/test_witness.py -q`";
        assert_eq!(
            parse_witness_command(text),
            Some("pytest tests/test_witness.py -q".to_string())
        );
    }

    #[test]
    fn marker_is_case_insensitive() {
        assert_eq!(
            parse_witness_command("test_command: go test ./pkg -run TestWitness"),
            Some("go test ./pkg -run TestWitness".to_string())
        );
    }

    #[test]
    fn missing_or_empty_marker_is_none_not_a_guess() {
        assert_eq!(parse_witness_command("no marker here"), None);
        assert_eq!(parse_witness_command("TEST_COMMAND:"), None);
        assert_eq!(parse_witness_command("TEST_COMMAND:   ``  "), None);
    }

    #[test]
    fn test_invocation_rejects_shell_operators_and_redirection() {
        for command in [
            "cargo test -p x && touch owned",
            "cargo test -p x || true",
            "cargo test -p x; touch owned",
            "cargo test -p x | tee results",
            "cargo test -p x > results",
            "cargo test -p x 2> results",
            "cargo test -p x < input",
            "cargo test -p $(touch owned)",
            "cargo test -p `touch owned`",
        ] {
            assert!(
                parse_test_invocation(command).is_err(),
                "shell syntax must be rejected: {command}"
            );
        }
    }

    #[test]
    fn test_invocation_parses_only_known_test_programs_into_argv() {
        assert_eq!(
            parse_test_invocation("cargo test -p 'my crate' witness -- --exact").unwrap(),
            TestInvocation {
                program: "cargo".into(),
                args: vec![
                    "test".into(),
                    "-p".into(),
                    "my crate".into(),
                    "witness".into(),
                    "--".into(),
                    "--exact".into(),
                ],
            }
        );
        assert!(parse_test_invocation("sh -c 'cargo test'").is_err());
        assert!(parse_test_invocation("python helper.py").is_err());
        assert!(parse_test_invocation("cargo build").is_err());
    }

    // ---- witness_watchlist ------------------------------------------------

    #[test]
    fn watchlist_is_created_and_modified_files_only() {
        let before = fps(&[("stale.txt", "a"), ("edited_test.rs", "old")]);
        let after = fps(&[
            ("stale.txt", "a"),         // untouched pre-existing dirt
            ("edited_test.rs", "new"),  // modified by the witness turn
            ("tests/witness.rs", "w1"), // created by the witness turn
        ]);
        let list = witness_watchlist(&before, &after);
        assert_eq!(list.len(), 2);
        assert_eq!(list.get("tests/witness.rs"), Some(&"w1".to_string()));
        assert_eq!(list.get("edited_test.rs"), Some(&"new".to_string()));
        assert!(!list.contains_key("stale.txt"));
    }

    #[test]
    fn accepted_witness_is_exactly_one_new_test_artifact() {
        let accepted = validate_witness_artifact(
            &fps(&[("src/lib.rs", "prod-v1")]),
            &fps(&[("src/lib.rs", "prod-v1")]),
            &HashMap::new(),
            &fps(&[("tests/authority_witness.rs", "sha256:whole-file")]),
        )
        .unwrap();
        assert_eq!(
            accepted,
            fps(&[("tests/authority_witness.rs", "sha256:whole-file")])
        );
    }

    #[test]
    fn witness_artifact_rejects_tracked_production_edits() {
        let error = validate_witness_artifact(
            &fps(&[("src/lib.rs", "prod-v1")]),
            &fps(&[("src/lib.rs", "prod-v2")]),
            &HashMap::new(),
            &fps(&[("tests/authority_witness.rs", "test")]),
        )
        .unwrap_err();
        assert!(error.to_string().contains("tracked"));
        assert!(error.to_string().contains("src/lib.rs"));
    }

    #[test]
    fn witness_artifact_rejects_non_test_and_pre_existing_mutations() {
        let non_test = validate_witness_artifact(
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &fps(&[
                ("tests/authority_witness.rs", "test"),
                ("README.md", "note"),
            ]),
        )
        .unwrap_err();
        assert!(non_test.to_string().contains("README.md"));

        let existing = validate_witness_artifact(
            &HashMap::new(),
            &HashMap::new(),
            &fps(&[("tests/authority_witness.rs", "old")]),
            &fps(&[("tests/authority_witness.rs", "new")]),
        )
        .unwrap_err();
        assert!(existing.to_string().contains("new test file"));
    }

    // ---- tampered_paths ----------------------------------------------------

    #[test]
    fn untouched_watchlist_reports_no_tampering() {
        let watch = fps(&[("tests/witness.rs", "w1")]);
        let current = fps(&[("tests/witness.rs", "w1"), ("other.rs", "x")]);
        assert!(tampered_paths(&watch, &current).is_empty());
    }

    #[test]
    fn a_modified_witness_file_is_tampered() {
        let watch = fps(&[("tests/witness.rs", "w1")]);
        let current = fps(&[("tests/witness.rs", "w2")]);
        assert_eq!(tampered_paths(&watch, &current), vec!["tests/witness.rs"]);
    }

    #[test]
    fn a_deleted_witness_file_is_tampered() {
        let watch = fps(&[("tests/witness.rs", "w1")]);
        let current = HashMap::new();
        assert_eq!(tampered_paths(&watch, &current), vec!["tests/witness.rs"]);
    }

    #[test]
    fn tampered_paths_are_sorted_for_deterministic_evidence() {
        let watch = fps(&[("b.rs", "1"), ("a.rs", "1")]);
        let current = HashMap::new();
        assert_eq!(tampered_paths(&watch, &current), vec!["a.rs", "b.rs"]);
    }

    // ---- prompts -----------------------------------------------------------

    #[test]
    fn witness_prompt_carries_goal_structure_recall_and_marker() {
        let recall = vec![RecalledFrame {
            citation_label: "memory: retries".to_string(),
            source: "memory".to_string(),
            content: "retry policy is deterministic".to_string(),
            token_cost: 4,
            id: None,
        }];
        let p = witness_prompt("fix the retry bug", &recall, "src/\n  lib.rs");
        assert!(p.contains("TEST_COMMAND:"));
        assert!(p.contains("fix the retry bug"));
        assert!(p.contains("src/"));
        assert!(p.contains("memory: retries"));
        assert!(p.contains("ONE NEW test file"));
    }

    #[test]
    fn repair_prompt_names_the_passing_command() {
        let p = witness_repair_prompt("cargo test -p x");
        assert!(p.contains("cargo test -p x"));
        assert!(p.contains("TEST_COMMAND:"));
    }
}
