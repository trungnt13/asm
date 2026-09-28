use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;

#[test]
fn enabled_update_setting_does_not_start_a_check_or_show_cached_upstream_version() {
    let home = tempfile::tempdir().expect("temporary Codex home");
    let config = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(
            ConfigBuilder::default()
                .codex_home(home.path().to_path_buf())
                .build(),
        )
        .expect("config");
    assert!(config.check_for_update_on_startup);
    // Without the ASM guard, a missing cache starts a task and panics here: no
    // runtime is active after config loading.
    assert_eq!(get_upgrade_version(&config), None);
    assert_eq!(get_upgrade_version_for_popup(&config), None);
    assert!(!version_filepath(&config).exists());
}
