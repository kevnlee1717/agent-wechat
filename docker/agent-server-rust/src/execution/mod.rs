pub mod actions;

use base64::Engine;
use crate::context::Context;
use crate::ia::{find_state_by_id, identify_states};
use crate::ia::types::*;
use crate::tools::a11y::get_a11y_desktop;
use crate::tools::exec::ExecOptions;
use crate::tools::screenshot::capture_screenshot;
use crate::effects::collect_effects;
use crate::db::get_db;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Only one plan can run at a time — they all drive the GUI.
static PLAN_LOCK: Mutex<()> = Mutex::const_new(());

pub struct ExecutionResult {
    pub success: bool,
    pub error: Option<String>,
}

const EXECUTION_TIMEOUT_MS: u64 = 300_000; // 5 minutes
const UNKNOWN_STATE_TIMEOUT_MS: u64 = 60_000; // 1 minute
const MAX_STEPS: u32 = 500;

/// Run the FSM execution loop with a generic plan.
///
/// 1. OBSERVE    → a11y tree + screenshot
/// 2. IDENTIFY   → match IAState from tree
/// 3. REDUCE     → update AppState via identified state's reduce()
/// 4. EFFECTS    → emit events on state change
/// 5. PERSIST    → save AppState to SQLite
/// 6. SELECT     → plan picks next action
/// 7. EXECUTE    → run action via tool scripts (emits fire inline)
/// 8. GOAL?      → plan checks if done
/// 9. LOOP
pub async fn run_execution_loop<P, PS, PA>(
    plan: &P,
    params: &PA,
    context: &mut Context,
    emit: &(dyn Fn(SubscriptionEvent) + Send + Sync),
    cancel: CancellationToken,
) -> (ExecutionResult, PS)
where
    P: crate::plans::Plan<PlanState = PS, Params = PA>,
    PS: Send,
    PA: Send,
{
    let _plan_guard = PLAN_LOCK.lock().await;

    // Pause health monitoring while an execution loop is active
    crate::sessions::health_monitor::pause_monitoring();
    struct ResumeOnDrop;
    impl Drop for ResumeOnDrop {
        fn drop(&mut self) {
            crate::sessions::health_monitor::resume_monitoring();
        }
    }
    let _health_guard = ResumeOnDrop;

    // RECEIVE_ONLY：确保 a11y 在跑（entrypoint 已在 WeChat 之前拉起，这里仅兜底）。
    // 注意（2026-06-15）：**不再执行结束就 stop**。长期掉线等登录的 WeChat 一旦 a11y bus 被销毁，其 atk-bridge
    // 注册永久失效且不重连 → 下次要二维码时 a11y unavailable，反复人工救。a11y bus 必须起一次永不销毁；
    // 常驻 idle CPU ≈ 0（churn 才耗 CPU）。故 StopA11yOnDrop 已废弃，stop_a11y 也改为 no-op。
    if crate::config::receive_only() {
        crate::tools::a11y_daemon::ensure_a11y_running(&context.session).await;
    }

    let mut plan_state = plan.initial_plan_state();
    let session_id = context.session_id.clone();

    let exec_options = ExecOptions {
        session: Some(context.session.clone()),
        timeout_ms: 60_000,
    };

    let execution_start = std::time::Instant::now();
    let mut unknown_state_since: Option<std::time::Instant> = None;
    let mut consecutive_a11y_failures: u32 = 0;
    const MAX_A11Y_FAILURES: u32 = 8;
    const A11Y_REPROBE_AT: u32 = 3;

    for step in 0..MAX_STEPS {
        // Check execution timeout
        if execution_start.elapsed().as_millis() as u64 > EXECUTION_TIMEOUT_MS {
            return (ExecutionResult {
                success: false,
                error: Some(format!(
                    "Execution timeout after {}s",
                    execution_start.elapsed().as_secs()
                )),
            }, plan_state);
        }

        if cancel.is_cancelled() {
            return (ExecutionResult {
                success: false,
                error: Some("Aborted".to_string()),
            }, plan_state);
        }

        // 1. OBSERVE: get a11y tree + screenshot
        let a11y_result = get_a11y_desktop(&exec_options).await;
        let a11y = match a11y_result {
            Ok(tree) => {
                consecutive_a11y_failures = 0;
                tree
            }
            Err(e) => {
                consecutive_a11y_failures += 1;
                tracing::warn!(
                    "[exec] a11y failed on step {step} ({}/{}): {e}",
                    consecutive_a11y_failures, MAX_A11Y_FAILURES
                );
                // RECEIVE_ONLY: a11y daemon may have died; try bringing it back once.
                if crate::config::receive_only() && consecutive_a11y_failures == A11Y_REPROBE_AT {
                    crate::tools::a11y_daemon::ensure_a11y_running(&context.session).await;
                }
                // Stop retrying forever if accessibility stays unavailable.
                if consecutive_a11y_failures >= MAX_A11Y_FAILURES {
                    tracing::error!(
                        "[exec] a11y unavailable after {} tries, aborting",
                        MAX_A11Y_FAILURES
                    );
                    emit(SubscriptionEvent {
                        event_type: "failed".to_string(),
                        data: [(
                            "message".to_string(),
                            serde_json::Value::String(
                                "Accessibility unavailable, login aborted".to_string(),
                            ),
                        )]
                        .into_iter()
                        .collect(),
                    });
                    return (
                        ExecutionResult {
                            success: false,
                            error: Some("a11y unavailable".to_string()),
                        },
                        plan_state,
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        let screenshot = capture_screenshot(&exec_options).await.unwrap_or_default();

        // 2. IDENTIFY: find current states
        let identified = identify_states(&a11y, &screenshot);

        if identified.main_window.is_none() {
            if unknown_state_since.is_none() {
                unknown_state_since = Some(std::time::Instant::now());
            }
            let elapsed = unknown_state_since.unwrap().elapsed();
            if elapsed.as_millis() as u64 > UNKNOWN_STATE_TIMEOUT_MS {
                tracing::error!("[exec] Unknown state timeout after {}s", elapsed.as_secs());
                return (ExecutionResult {
                    success: false,
                    error: Some(format!(
                        "Unknown state for {}s - no matching IAState found",
                        elapsed.as_secs()
                    )),
                }, plan_state);
            }
            tracing::warn!("[exec] Unknown state ({}s), waiting...", elapsed.as_secs());
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            continue;
        }

        unknown_state_since = None;

        // Log identified states
        tracing::info!(
            "[exec] step={step} mainWindow={}, popup={}, contactCard={}, settings={}",
            identified
                .main_window
                .as_ref()
                .map(|s| s.state_id.as_str())
                .unwrap_or("none"),
            identified
                .popup
                .as_ref()
                .map(|s| s.state_id.as_str())
                .unwrap_or("none"),
            identified
                .contact_card
                .as_ref()
                .map(|s| s.state_id.as_str())
                .unwrap_or("none"),
            identified
                .settings
                .as_ref()
                .map(|s| s.state_id.as_str())
                .unwrap_or("none"),
        );

        // 3. REDUCE: update AppState via identified state reduce()
        let prev_state = context.state.clone();
        let screenshot_bytes = base64::engine::general_purpose::STANDARD
            .decode(&screenshot)
            .unwrap_or_default();

        if let Some(ref mw) = identified.main_window {
            if let Some(state_impl) = find_state_by_id(&mw.state_id) {
                context.state = state_impl.reduce(&ReduceArgs {
                    prev: &prev_state,
                    a11y: &a11y,
                    screenshot: &screenshot_bytes,
                });
            }
        }

        if let Some(ref popup) = identified.popup {
            if let Some(state_impl) = find_state_by_id(&popup.state_id) {
                let current = context.state.clone();
                context.state = state_impl.reduce(&ReduceArgs {
                    prev: &current,
                    a11y: &a11y,
                    screenshot: &screenshot_bytes,
                });
            }
        } else {
            context.state.popup = None;
        }

        if let Some(ref cc) = identified.contact_card {
            if let Some(state_impl) = find_state_by_id(&cc.state_id) {
                let current = context.state.clone();
                context.state = state_impl.reduce(&ReduceArgs {
                    prev: &current,
                    a11y: &a11y,
                    screenshot: &screenshot_bytes,
                });
            }
        } else {
            context.state.contact_card = None;
        }

        if let Some(ref s) = identified.settings {
            if let Some(state_impl) = find_state_by_id(&s.state_id) {
                let current = context.state.clone();
                context.state = state_impl.reduce(&ReduceArgs {
                    prev: &current,
                    a11y: &a11y,
                    screenshot: &screenshot_bytes,
                });
            }
        } else {
            context.state.settings = None;
        }

        // 4. EFFECTS
        let effects = collect_effects(&prev_state, &context.state);
        for effect in effects {
            match effect {
                Effect::Emit { event } => emit(event),
            }
        }

        // 5. PERSIST
        {
            let db = get_db();
            context.save(&db);
        }

        // 6. SELECT: plan picks next action
        let selected = plan
            .select_action(
                &context.state,
                params,
                &identified,
                &mut plan_state,
                &a11y,
                &session_id,
            )
            .await;

        tracing::debug!(
            "[exec] selected action: {}",
            selected
                .as_ref()
                .map(|s| format!("{:?}", s.action))
                .unwrap_or_else(|| "none".to_string())
        );

        // 7. EXECUTE: run the action (emits fire inline via callback)
        if let Some(sel) = &selected {
            actions::execute_action(&sel.action, sel.frame.as_ref(), &exec_options, &a11y, emit).await;
        }

        // 8. GOAL CHECK (after action)
        if plan.is_goal_reached(&context.state, &plan_state) {
            return (ExecutionResult {
                success: true,
                error: None,
            }, plan_state);
        }

        // No action = stuck (only if plan returns None)
        if selected.is_none() {
            return (ExecutionResult {
                success: false,
                error: Some("No action selected".to_string()),
            }, plan_state);
        }
    }

    (ExecutionResult {
        success: false,
        error: Some("Max steps reached".to_string()),
    }, plan_state)
}
