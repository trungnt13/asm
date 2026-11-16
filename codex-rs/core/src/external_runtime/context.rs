use codex_protocol::models::ResponseItem;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use codex_async_utils::OrCancelExt;
use tokio_util::sync::CancellationToken;

use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn;
use crate::session::turn_context::TurnContext;

pub(super) struct PreparedContext {
    pub(super) step: Arc<StepContext>,
    injections: Vec<ResponseItem>,
    connectors: HashSet<String>,
}

#[tracing::instrument(level = "trace", skip_all, err)]
pub(super) async fn capture(
    session: &Arc<Session>,
    turn: Arc<TurnContext>,
    input: &[TurnInput],
    cancellation: &CancellationToken,
) -> anyhow::Result<PreparedContext> {
    let user_input = turn::turn_user_input(input);
    let (required_servers, mentioned_plugins) =
        turn::required_mcp_servers_for_input(session, &turn, &user_input)
            .or_cancel(cancellation)
            .await
            .map_err(|_| anyhow::anyhow!("ASM plugin preparation was cancelled"))?;
    let required_plugins = crate::plugins::collect_explicit_plugin_ids(&user_input);
    let step = session
        .capture_step_context_with_required_mcp_servers(
            turn,
            cancellation,
            &required_servers,
            &required_plugins,
        )
        .await?;
    let (injections, connectors) = turn::build_skills_and_plugins(
        session,
        &step,
        &user_input,
        &mentioned_plugins,
        cancellation,
    )
    .await
    .context("ASM skill and plugin preparation was cancelled")?;
    Ok(PreparedContext {
        step,
        injections,
        connectors,
    })
}

#[tracing::instrument(level = "trace", skip_all, err)]
pub(super) async fn record(session: &Session, prepared: PreparedContext) -> anyhow::Result<()> {
    let PreparedContext {
        step,
        injections,
        connectors,
    } = prepared;
    session
        .record_context_updates_and_set_reference_context_item(&step)
        .await?;
    session.merge_connector_selection(connectors).await;
    if !injections.is_empty() {
        session
            .record_conversation_items(&step.turn, &step.settings.model_info, &injections)
            .await;
    }
    Ok(())
}
