//! Complete per-call accounting for pipeline roles that call providers directly.

use std::time::{Duration, Instant};

use stella_core::BudgetGuard;
use stella_core::retry::{RetryPolicy, retry_with_backoff};
use stella_protocol::{
    AgentEvent, CompletionMessage, CompletionRequest, CompletionResult, ModelCallRole,
    UsageIncompleteReason,
};
use tokio::time::timeout;

use super::stage_budget::{PipelineBudgetAbort, record_and_tick};
use super::{Pipeline, ResolvedRole, RoleCallOverrides};

pub(super) struct RawCall<'r, 'a> {
    pub(super) role: ModelCallRole,
    pub(super) resolved: &'r ResolvedRole<'a>,
    pub(super) messages: Vec<CompletionMessage>,
    pub(super) policy: RetryPolicy,
    pub(super) overrides: &'r RoleCallOverrides,
    pub(super) timeout: Option<Duration>,
}

pub(super) enum RawCallError {
    Provider,
    Timeout,
    Budget(PipelineBudgetAbort),
}

impl<'a> Pipeline<'a> {
    /// One metered raw provider completion. Successful calls emit exactly one
    /// `StepUsage` before budget enforcement can return; failures/timeouts emit
    /// one content-free `UsageIncomplete`. All raw roles use this chokepoint.
    pub(super) async fn metered_raw_call(
        &self,
        call: RawCall<'_, 'a>,
        budget: &mut BudgetGuard,
        total: &mut f64,
    ) -> Result<CompletionResult, RawCallError> {
        let messages = match &call.overrides.prompt {
            Some(prompt) => {
                let mut with_system = Vec::with_capacity(call.messages.len() + 1);
                with_system.push(CompletionMessage::system(prompt.clone()));
                with_system.extend(call.messages.clone());
                with_system
            }
            None => call.messages.clone(),
        };
        let engine = &self.config.engine;
        let req = CompletionRequest {
            messages,
            max_output_tokens: call
                .overrides
                .max_output_tokens
                .or(engine.max_output_tokens),
            temperature: call.overrides.temperature.or(engine.temperature),
            effort: call.overrides.effort.or(engine.effort),
            reasoning: call.overrides.reasoning.or(engine.reasoning),
            params: call.overrides.params.or(engine.params),
            tools: Vec::new(),
        };
        let started = Instant::now();
        let future = retry_with_backoff(&call.policy, self.sleeper, || {
            call.resolved.provider.complete(req.clone())
        });
        let outcome = match call.timeout {
            Some(limit) => match timeout(limit, future).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(error)) => {
                    self.emit_raw_incomplete(
                        &call,
                        UsageIncompleteReason::ProviderError,
                        started.elapsed(),
                        Some(if error.is_retryable() {
                            call.policy.max_retries
                        } else {
                            0
                        }),
                    );
                    return Err(RawCallError::Provider);
                }
                Err(_) => {
                    self.emit_raw_incomplete(
                        &call,
                        UsageIncompleteReason::Timeout,
                        started.elapsed(),
                        None,
                    );
                    return Err(RawCallError::Timeout);
                }
            },
            None => match future.await {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.emit_raw_incomplete(
                        &call,
                        UsageIncompleteReason::ProviderError,
                        started.elapsed(),
                        Some(if error.is_retryable() {
                            call.policy.max_retries
                        } else {
                            0
                        }),
                    );
                    return Err(RawCallError::Provider);
                }
            },
        };
        for attempt in &outcome.retries {
            self.emit(AgentEvent::Retry {
                attempt: attempt.attempt,
                reason: attempt.reason.clone(),
            });
        }
        let result = outcome.value;
        let provider = call.resolved.provider.id();
        self.emit(AgentEvent::StepUsage {
            step: 0,
            role: call.role,
            provider: provider.to_string(),
            model: result.model.clone(),
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            cached_input_tokens: result.usage.cached_input_tokens,
            cache_write_tokens: result.usage.cache_write_tokens,
            estimated_input_tokens: 0,
            cost_usd: result.cost_usd,
            duration_ms: started.elapsed().as_millis() as u64,
            retries: outcome.retries.len() as u32,
            tool_calls: result.tool_calls.len(),
            complete: result.usage.is_complete_for(provider),
        });
        *total += result.cost_usd;
        record_and_tick(budget, result.cost_usd, &self.events).map_err(RawCallError::Budget)?;
        Ok(result)
    }

    fn emit_raw_incomplete(
        &self,
        call: &RawCall<'_, 'a>,
        reason: UsageIncompleteReason,
        duration: Duration,
        retries: Option<u32>,
    ) {
        self.emit(AgentEvent::UsageIncomplete {
            role: call.role,
            provider: call.resolved.provider.id().to_string(),
            model: call.resolved.model_ref.model_id.clone(),
            reason,
            duration_ms: duration.as_millis() as u64,
            retries,
        });
    }
}
