//! Candidate-local witness authoring for the isolated typed-execution boundary.

use super::*;

impl<'a> Pipeline<'a> {
    /// Author one failing witness inside the same disposable candidate that
    /// will execute, revise, verify, and (only on pass) adopt it.
    pub(super) async fn witness_stage(
        &self,
        goal: &str,
        frames: &[RecalledFrame],
        tools: &dyn stella_core::ToolExecutor,
        surface: CandidateSurface<'_>,
        budget: &mut BudgetGuard,
        total: &mut f64,
    ) -> Result<Option<Witness>, String> {
        self.emit(AgentEvent::Stage {
            name: StageKind::Witness,
        });
        let resolved = match self
            .resolve_provider(Role::Judge)
            .or_else(|_| self.resolve_provider(Role::Worker))
        {
            Ok(resolved) => resolved,
            Err(_) => return Err("could not resolve an independent witness author".to_string()),
        };
        if let Some(fallback) = &resolved.fallback {
            self.emit_fallback(fallback);
        }

        let tracked_before = surface.repo_status.tracked_fingerprints().await;
        let untracked_before = surface.repo_status.untracked_fingerprints().await;
        let structure = self.repo.structure_summary().await;
        let mut engine = Engine::with_sleeper(
            resolved.provider,
            tools,
            self.config.engine.clone(),
            self.sleeper,
        );
        if let Some((hooks, runner)) = self.hooks {
            engine = engine.with_hooks(hooks, runner);
        }

        let mut messages = vec![
            CompletionMessage::system(WITNESS_SYSTEM_PROMPT),
            CompletionMessage::user(witness_prompt(goal, frames, &structure)),
        ];
        let mut file_changes = 0u32;
        let text = match self
            .run_engine_turn(&engine, &mut messages, budget, &mut file_changes)
            .await
        {
            TurnOutcome::Completed { text, cost_usd } => {
                *total += cost_usd;
                text
            }
            TurnOutcome::Aborted { reason, cost_usd } => {
                *total += cost_usd;
                if let Some(abort) = budget_abort(budget.evaluate()) {
                    return Err(abort.reason);
                }
                return Err(format!("witness author turn aborted: {reason}"));
            }
        };
        let Some(mut command) = parse_witness_command(&text) else {
            return Err("witness author produced no TEST_COMMAND line".to_string());
        };

        let Ok(mut invocation) = parse_test_invocation(&command) else {
            return Err(format!(
                "witness author produced an unsafe or unsupported test command `{command}`"
            ));
        };
        if surface.tests.run_test(&invocation).await.passed() {
            messages.push(CompletionMessage::user(witness_repair_prompt(&command)));
            let repaired = match self
                .run_engine_turn(&engine, &mut messages, budget, &mut file_changes)
                .await
            {
                TurnOutcome::Completed { text, cost_usd } => {
                    *total += cost_usd;
                    text
                }
                TurnOutcome::Aborted { reason, cost_usd } => {
                    *total += cost_usd;
                    if let Some(abort) = budget_abort(budget.evaluate()) {
                        return Err(abort.reason);
                    }
                    return Err(format!("witness repair turn aborted: {reason}"));
                }
            };
            command = parse_witness_command(&repaired).unwrap_or(command);
            let Ok(repaired_invocation) = parse_test_invocation(&command) else {
                return Err(format!(
                    "witness repair produced an unsafe or unsupported test command `{command}`"
                ));
            };
            invocation = repaired_invocation;
            if surface.tests.run_test(&invocation).await.passed() {
                return Err(
                    "witness test still passes on the unmodified code after one repair".to_string(),
                );
            }
        }

        let tracked_after = surface.repo_status.tracked_fingerprints().await;
        let untracked_after = surface.repo_status.untracked_fingerprints().await;
        let files = validate_witness_artifact(
            &tracked_before,
            &tracked_after,
            &untracked_before,
            &untracked_after,
        )
        .map_err(|error| error.to_string())?;
        Ok(Some(Witness {
            command,
            invocation,
            files,
        }))
    }
}
