use super::config_processor::map_error as map_config_error;
use crate::config_manager::ConfigManager;
use crate::error_code::internal_error;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteErrorCode;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::MergeStrategy;
use codex_core::config::Config;
use codex_core::config::edit::ConfigEditsBuilder;

pub(super) async fn clear_user_model_provider_if_bedrock(
    config_manager: &ConfigManager,
    config: &Config,
) -> Result<(), JSONRPCErrorError> {
    let mut latest_config = None;
    loop {
        let config = latest_config.as_ref().unwrap_or(config);
        let Some(key_paths) = ConfigEditsBuilder::bedrock_provider_config_paths_to_clear(config)
        else {
            return Ok(());
        };
        let expected_version = config
            .config_layer_stack
            .get_active_user_layer()
            .map(|layer| layer.version.clone());

        let result = config_manager
            .batch_write(ConfigBatchWriteParams {
                edits: key_paths
                    .into_iter()
                    .map(|segments| ConfigEdit {
                        key_path: segments.join("."),
                        value: serde_json::Value::Null,
                        merge_strategy: MergeStrategy::Replace,
                    })
                    .collect(),
                file_path: None,
                expected_version,
                reload_user_config: false,
            })
            .await;
        if let Err(err) = &result
            && latest_config.is_none()
            && err.write_error_code() == Some(ConfigWriteErrorCode::ConfigVersionConflict)
        {
            tracing::warn!(
                "configuration changed while clearing Amazon Bedrock settings; retrying once"
            );
            latest_config = Some(
                config_manager
                    .load_latest_config(/*fallback_cwd*/ None)
                    .await
                    .map_err(|err| {
                        internal_error(format!(
                            "failed to reload Amazon Bedrock configuration: {err}"
                        ))
                    })?,
            );
            continue;
        }
        return result.map(|_| ()).map_err(map_config_error);
    }
}
