//! The deck's async run loop — the multi-agent analogue of [`crate::shell::run`].
//!
//! Deliberately thin, like the single-session shell: every decision
//! (key→action via [`crate::deck_ui`], event→state via
//! [`crate::deck_ui::ingest_inbound`], the frame via [`crate::deck_render`])
//! lives in pure, unit-tested layers. This file only wires them to real I/O.
//!
//! It differs from [`crate::shell::run`] in one structural way: a fixed
//! **animation/resource tick** (~30 fps) is a third `select!` arm. A live
//! dashboard — CPU gauges, elapsed timers, sparklines, tachyonfx transitions —
//! must repaint on a clock, not only when the agent streams. That tick is also
//! where the clock advances and the resource monitor samples, so all
//! time-based UI shares one heartbeat.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::composer::{Composer, SlashCommand};
use crate::deck::WorkspaceModel;
use crate::deck_render::render_deck;
use crate::deck_ui::{DeckAction, DeckUi, handle_deck_key, ingest_inbound};
use crate::envelope::{AgentMeta, AgentStatus, Inbound, WorkspaceInput};
use crate::graph::GraphSnapshot;
use crate::resource::ResourceMonitor;
use crate::shell::DebugLog;

/// The repaint / sample cadence. ~30 fps keeps animations smooth and the CPU
/// gauge / elapsed timers live without busy-spinning.
const TICK: Duration = Duration::from_millis(33);

/// The synthetic agent id `!` shell commands run under — they get their own
/// dashboard lane and transcript instead of polluting a real agent's fold.
const SHELL_AGENT: &str = "shell";

/// Cap on captured shell output fed back as an event. Head and tail are both
/// kept (errors live at the tail); the middle is elided.
const SHELL_OUTPUT_CAP: usize = 4000;

/// Configuration for one deck session.
#[derive(Debug, Clone, Default)]
pub struct DeckOptions {
    /// Enable mouse capture (comfy-tabs click/scroll/reorder). Off by default so
    /// native terminal selection keeps working (L-T2).
    pub mouse_capture: bool,
    /// Structured debug log path (`OXAGEN_DEBUG=1`), or `None` for a no-op sink.
    pub debug_log_path: Option<PathBuf>,
    /// An initial code-graph snapshot to seed the Graph tab (the caller, which
    /// owns a `CodeGraph`, queries it and hands it in — the TUI stays decoupled).
    pub initial_graph: Option<GraphSnapshot>,
    /// The slash-command vocabulary for the `/` popup (the caller owns the
    /// real list, exactly like the single-session `RunOptions`).
    pub slash_commands: Vec<SlashCommand>,
}

/// Restores the terminal on drop, including during a panic unwind.
///
/// Each terminal state is flagged as it is acquired, and the guard exists
/// BEFORE the first acquisition — so an error partway through `enter` (raw
/// mode on, alternate screen failed) still drops the guard and rolls back
/// exactly the states that were entered, never stranding the user's terminal
/// in raw mode.
struct TerminalGuard {
    raw: bool,
    alt: bool,
    mouse: bool,
}

impl TerminalGuard {
    fn enter(mouse: bool) -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            alt: false,
            mouse: false,
        };
        let mut out = io::stdout();
        enable_raw_mode()?;
        guard.raw = true;
        execute!(out, EnterAlternateScreen)?;
        guard.alt = true;
        if mouse {
            execute!(out, EnableMouseCapture)?;
            guard.mouse = true;
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if self.mouse {
            let _ = execute!(out, DisableMouseCapture);
        }
        if self.alt {
            let _ = execute!(out, LeaveAlternateScreen);
        }
        if self.raw {
            let _ = disable_raw_mode();
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Run one `!` shell command **immediately** on the local event lane.
///
/// The command gets the synthetic [`SHELL_AGENT`] lane: a `Register` (idempotent
/// — re-registering only refreshes the title to the latest command), a
/// `ToolStart` so the invocation is visible the instant it launches, and a
/// `ToolResult` + terminal `Status` when it finishes. stdout and stderr are
/// both captured; a non-zero exit reports as a tool error. The TUI never
/// blocks on the child — it runs on a spawned task and reports back over `tx`.
fn spawn_shell_command(cmd: String, tx: UnboundedSender<Inbound>, started_ms: u64) {
    use stella_protocol::{AgentEvent, ToolCall, ToolOutput};

    let call_id = format!("shell-{started_ms}");
    let _ = tx.send(Inbound::Register(
        AgentMeta::new(SHELL_AGENT, format!("! {cmd}"), started_ms).with_role("shell"),
    ));
    let _ = tx.send(Inbound::Event {
        agent: SHELL_AGENT.to_string(),
        event: AgentEvent::ToolStart {
            call: ToolCall {
                call_id: call_id.clone(),
                name: "shell".to_string(),
                input: serde_json::json!({ "cmd": cmd }),
            },
        },
    });

    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .await;
        let (ok, content) = match result {
            Ok(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                if !out.stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                }
                if text.trim().is_empty() {
                    text = format!("(no output — exit {})", out.status);
                }
                (out.status.success(), cap_output(&text))
            }
            Err(e) => (false, format!("failed to spawn `sh -c`: {e}")),
        };
        let output = if ok {
            ToolOutput::Ok { content }
        } else {
            ToolOutput::Error { message: content }
        };
        let _ = tx.send(Inbound::Event {
            agent: SHELL_AGENT.to_string(),
            event: AgentEvent::ToolResult {
                call_id,
                output,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        });
        // Park the lane so it never reads as still-working (a lingering
        // Running shell agent would keep the spinner alive forever).
        let _ = tx.send(Inbound::Status {
            agent: SHELL_AGENT.to_string(),
            status: if ok {
                AgentStatus::Done
            } else {
                AgentStatus::Failed
            },
        });
    });
}

/// Middle-out cap on shell output: keep the head and the tail (errors live at
/// the tail), elide the middle past [`SHELL_OUTPUT_CAP`] chars.
fn cap_output(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= SHELL_OUTPUT_CAP {
        return text.to_string();
    }
    let half = SHELL_OUTPUT_CAP / 2;
    let head: String = chars[..half].iter().collect();
    let tail: String = chars[chars.len() - half..].iter().collect();
    format!("{head}\n…[output truncated]…\n{tail}")
}


/// Run the command deck to completion. [`Inbound`] envelopes stream in over
/// `inbound`; the user's [`WorkspaceInput`]s stream out over `submissions`.
/// Returns when the inbound stream closes or the user quits, having always
/// restored the terminal first.
pub async fn run_deck(
    opts: DeckOptions,
    mut inbound: UnboundedReceiver<Inbound>,
    submissions: UnboundedSender<WorkspaceInput>,
) -> io::Result<()> {
    let debug = DebugLog::new(opts.debug_log_path.clone());
    debug.note("deck session start");

    let _guard = TerminalGuard::enter(opts.mouse_capture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut model = WorkspaceModel::new();
    model.now_ms = now_ms();
    let mut ui = DeckUi::new(Composer::new());
    ui.graph = opts.initial_graph.clone();
    ui.slash_commands = opts.slash_commands.clone();
    let mut resources = ResourceMonitor::new();

    // Synthetic-event lane for `!` shell commands: spawned commands report
    // back here and are folded exactly like engine events. The sender lives
    // for the whole loop, so this arm never closes it.
    let (local_tx, mut local_rx) = tokio::sync::mpsc::unbounded_channel::<Inbound>();

    // Blocking crossterm reader → async loop, with a shutdown flag.
    let (key_tx, mut key_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let reader_shutdown = shutdown.clone();
    let reader = std::thread::spawn(move || {
        while !reader_shutdown.load(Ordering::Relaxed) {
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => match event::read() {
                    Ok(ev) => {
                        if key_tx.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });

    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    'run: loop {
        terminal.draw(|f| render_deck(&model, &mut ui, f))?;

        tokio::select! {
            maybe_inbound = inbound.recv() => {
                match maybe_inbound {
                    Some(ev) => ingest_inbound(&ev, &mut model, &mut ui),
                    // The engine closed the stream — session over.
                    None => break 'run,
                }
            }
            maybe_key = key_rx.recv() => {
                match maybe_key {
                    Some(Event::Key(key)) if key.kind != KeyEventKind::Release => {
                        match handle_deck_key(key, &model, &mut ui) {
                            DeckAction::Quit => {
                                debug.note("user quit");
                                let _ = submissions.send(WorkspaceInput::Quit);
                                break 'run;
                            }
                            DeckAction::Send(input) => {
                                // Queue edits are reflected locally so they show
                                // immediately, then forwarded for dispatch — the
                                // input path never blocks on a busy agent. (The
                                // queue is the labeled out-of-band fold of the
                                // OUTBOUND stream; this is its one mutation site.)
                                match &input {
                                    WorkspaceInput::Enqueue { text } => {
                                        model.queue.enqueue(text.clone(), model.now_ms);
                                    }
                                    WorkspaceInput::QueueRemove { index } => {
                                        model.queue.remove(*index);
                                    }
                                    WorkspaceInput::QueueClear => model.queue.clear(),
                                    _ => {}
                                }
                                let _ = submissions.send(input);
                            }
                            DeckAction::Shell(cmd) => {
                                // `!` commands run NOW — never queued, never
                                // waiting on the engine. Output returns on the
                                // local lane as ordinary events.
                                debug.note(&format!("shell: {cmd}"));
                                spawn_shell_command(cmd, local_tx.clone(), model.now_ms);
                            }
                            DeckAction::Handled | DeckAction::Ignored => {}
                        }
                    }
                    // Resize / mouse / paste: the next draw picks them up.
                    Some(_) => {}
                    // Reader thread ended (stdin closed).
                    None => break 'run,
                }
            }
            maybe_local = local_rx.recv() => {
                // Shell-command lane (see `spawn_shell_command`). `local_tx`
                // outlives the loop, so `None` cannot actually occur.
                if let Some(ev) = maybe_local {
                    ingest_inbound(&ev, &mut model, &mut ui);
                }
            }
            _ = tick.tick() => {
                // The heartbeat: advance the clock and re-sample resources so
                // gauges, elapsed timers, sparklines, and effects stay live.
                model.now_ms = now_ms();
                resources.sample(&mut model);
            }
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    let _ = reader.join();
    debug.note("deck session end");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_output_keeps_head_and_tail() {
        let short = "fits";
        assert_eq!(cap_output(short), short);
        let long = format!("HEAD{}TAIL", "x".repeat(SHELL_OUTPUT_CAP * 2));
        let capped = cap_output(&long);
        assert!(capped.starts_with("HEAD"));
        assert!(capped.ends_with("TAIL"));
        assert!(capped.contains("[output truncated]"));
        assert!(capped.chars().count() < long.chars().count());
    }

    #[test]
    fn shell_commands_report_on_the_local_lane() {
        // The spawner's synchronous part: Register + ToolStart land on the
        // channel immediately, before the child even runs.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_shell_command("echo hi".into(), tx, 42);
        match rx.try_recv() {
            Ok(Inbound::Register(meta)) => {
                assert_eq!(meta.id, SHELL_AGENT);
                assert!(meta.title.contains("echo hi"));
            }
            other => panic!("expected Register first, got {other:?}"),
        }
        match rx.try_recv() {
            Ok(Inbound::Event { agent, .. }) => assert_eq!(agent, SHELL_AGENT),
            other => panic!("expected ToolStart second, got {other:?}"),
        }
        // The async completion (ToolResult + terminal Status) needs the
        // runtime to run the child; the sync part above is the determinism
        // this test pins, so completion just needs to arrive eventually.
        rt.block_on(async {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
        });
    }
}
