//! `build_project` and `run_tests` — toolchain-aware build/test execution.
//!
//! Both are thin verb shortcuts over the project scripts index
//! (`crate::scripts`, spec: `docs/design/scripts-index.md`): detection is
//! the index's one code path, `build_project` runs the `build` verb
//! binding, and `run_tests` layers its `kind` (unit / e2e / all) and
//! `filter` semantics on top — mapped to the runner's native filtering
//! flag, or to the project's own `test:unit`/`test:e2e` scripts. An
//! explicit `command` override still bypasses detection for anything
//! exotic.

use async_trait::async_trait;
use serde_json::Value;
use stella_protocol::tool::{ToolOutput, ToolSchema};

use crate::exec::run_and_report;
use crate::registry::Tool;
use crate::scripts::ScriptIndex;

const DEFAULT_TIMEOUT_SECS: u64 = 600;

fn no_toolchain_error() -> ToolOutput {
    ToolOutput::Error {
        message: "no recognized toolchain (looked for Cargo.toml, package.json, deno.json, \
                  pyproject.toml, go.mod, Makefile, justfile, Taskfile.yml, composer.json) — \
                  pass `command` explicitly"
            .into(),
    }
}

pub struct BuildProject;

#[async_trait]
impl Tool for BuildProject {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "build_project".into(),
            description: "Build the workspace with its own toolchain (cargo/npm/go/make/…), or \
                          a custom command."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Override the detected build command" },
                    "timeout_secs": { "type": "integer" }
                }
            }),
            read_only: false,
        }
    }

    async fn execute(&self, input: &Value, root: &std::path::Path) -> ToolOutput {
        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
            return run_and_report(command, root, timeout_secs).await;
        }
        let index = ScriptIndex::detect(root).await;
        if index.is_empty() {
            return no_toolchain_error();
        }
        match index.verb_entry("build") {
            Some(entry) => run_and_report(&entry.command, root, timeout_secs).await,
            None => ToolOutput::Error {
                message: "no `build` script detected in this workspace (see list_scripts) — \
                          pass `command`"
                    .into(),
            },
        }
    }
}

pub struct RunTests;

#[async_trait]
impl Tool for RunTests {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "run_tests".into(),
            description: "Run tests with the workspace's own runner. kind: unit|e2e|all. \
                          filter: module, package, file, or test name (runner-native)."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["unit", "e2e", "all"] },
                    "filter": { "type": "string", "description": "Narrow to a module/file/test" },
                    "command": { "type": "string", "description": "Override the detected test command" },
                    "timeout_secs": { "type": "integer" }
                }
            }),
            read_only: false,
        }
    }

    async fn execute(&self, input: &Value, root: &std::path::Path) -> ToolOutput {
        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
            return run_and_report(command, root, timeout_secs).await;
        }
        let kind = input.get("kind").and_then(|v| v.as_str()).unwrap_or("all");
        let filter = input.get("filter").and_then(|v| v.as_str()).unwrap_or("");

        let index = ScriptIndex::detect(root).await;
        let Some(primary) = index.primary_runner() else {
            return no_toolchain_error();
        };
        let command = match primary {
            "cargo" => match kind {
                // Unit tests live beside the code; e2e = the integration
                // test targets under tests/.
                "unit" => format!("cargo test --workspace --lib --bins {filter}"),
                "e2e" => {
                    if filter.is_empty() {
                        "cargo test --workspace --test '*'".to_string()
                    } else {
                        format!("cargo test --workspace --test '*' {filter}")
                    }
                }
                _ => format!("cargo test --workspace {filter}"),
            },
            pm @ ("npm" | "pnpm" | "yarn" | "bun") => {
                let scripts = index.root_script_names(pm);
                let script = match kind {
                    "unit" if scripts.contains("test:unit") => "test:unit",
                    "e2e" if scripts.contains("test:e2e") => "test:e2e",
                    "e2e" if scripts.contains("e2e") => "e2e",
                    // NO generic-`test` fallback for e2e: running unit tests
                    // while reporting "e2e passed" is a lie.
                    "unit" | "all" if scripts.contains("test") => "test",
                    _ => {
                        return ToolOutput::Error {
                            message: format!(
                                "package.json has no matching test script for kind `{kind}` — \
                                 pass `command`"
                            ),
                        };
                    }
                };
                if filter.is_empty() {
                    format!("{pm} run {script}")
                } else {
                    format!("{pm} run {script} -- {filter}")
                }
            }
            "go" => {
                if filter.is_empty() {
                    "go test ./...".to_string()
                } else {
                    format!("go test ./... -run {filter}")
                }
            }
            runner => {
                // uv/poetry/make/just/task/composer/deno: the project's own
                // e2e script or nothing (same no-lying rule as above); unit
                // and all ride the index's `test` verb binding. pytest takes
                // the filter as a positional; the task runners have no
                // native filter flag, so a filter there is ignored.
                let scripts = index.root_script_names(runner);
                if kind == "e2e" {
                    let script = ["test:e2e", "e2e"]
                        .into_iter()
                        .find(|s| scripts.contains(s));
                    match script.and_then(|s| index.resolve(s, Some(".")).ok()) {
                        Some(entry) => entry.command.clone(),
                        None => {
                            return ToolOutput::Error {
                                message: "no e2e test script detected for kind `e2e` — pass \
                                          `command`"
                                    .into(),
                            };
                        }
                    }
                } else {
                    match index.verb_entry("test") {
                        Some(entry) if matches!(runner, "uv" | "poetry") && !filter.is_empty() => {
                            format!("{} {filter}", entry.command)
                        }
                        Some(entry) => entry.command.clone(),
                        None => {
                            return ToolOutput::Error {
                                message: "no test script detected in this workspace (see \
                                          list_scripts) — pass `command`"
                                    .into(),
                            };
                        }
                    }
                }
            }
        };
        run_and_report(&command, root, timeout_secs).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_toolchain_is_a_named_error_and_command_overrides() {
        let root = tempfile::tempdir().unwrap();
        let out = BuildProject
            .execute(&serde_json::json!({}), root.path())
            .await;
        assert!(out.is_error());

        let out = BuildProject
            .execute(
                &serde_json::json!({"command": "echo built && exit 0"}),
                root.path(),
            )
            .await;
        assert!(!out.is_error(), "{out:?}");

        let out = RunTests
            .execute(&serde_json::json!({"command": "exit 1"}), root.path())
            .await;
        match &out {
            ToolOutput::Error { message } => assert!(message.contains("FAILED"), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn node_detection_uses_scripts_and_pm_lockfile() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("package.json"),
            r#"{"scripts": {"test": "echo node-tests-ran", "build": "echo node-build-ran"}}"#,
        )
        .unwrap();
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").unwrap();

        // pnpm may not exist in the test environment — we only assert the
        // detection path constructs the right command shape, visible in the
        // success/error text either way.
        let out = RunTests
            .execute(&serde_json::json!({"kind": "all"}), root.path())
            .await;
        let text = match &out {
            ToolOutput::Ok { content } => content.clone(),
            ToolOutput::Error { message } => message.clone(),
        };
        assert!(text.contains("pnpm run test"), "{text}");

        let out = RunTests
            .execute(&serde_json::json!({"kind": "e2e"}), root.path())
            .await;
        assert!(out.is_error(), "no e2e script → named error: {out:?}");
    }

    #[tokio::test]
    async fn make_targets_drive_build_and_tests_via_the_index() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("Makefile"),
            "build:\n\t@echo make-build-ran\ntest:\n\t@echo make-test-ran\n",
        )
        .unwrap();

        let out = BuildProject
            .execute(&serde_json::json!({}), root.path())
            .await;
        match &out {
            ToolOutput::Ok { content } => assert!(content.contains("make-build-ran"), "{content}"),
            other => panic!("{other:?}"),
        }

        let out = RunTests
            .execute(&serde_json::json!({"kind": "all"}), root.path())
            .await;
        match &out {
            ToolOutput::Ok { content } => assert!(content.contains("make-test-ran"), "{content}"),
            other => panic!("{other:?}"),
        }

        // A bare `test` target must NOT pass itself off as e2e — the same
        // no-lying rule the npm-family mapping enforces.
        let out = RunTests
            .execute(&serde_json::json!({"kind": "e2e"}), root.path())
            .await;
        assert!(out.is_error(), "no e2e target → named error: {out:?}");
    }
}
