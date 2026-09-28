use super::*;
use codex_tui::DaemonUpdateSource;
use pretty_assertions::assert_eq;
use std::os::unix::fs::PermissionsExt;

#[test]
fn update_actions_never_start_an_updater() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let executable = dir.path().join("fake CLI");
    let receipt = dir.path().join("executed");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\n", receipt.display()),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(/*mode*/ 0o700))?;
    for action in [
        UpdateAction::Daemon(DaemonUpdateSource::PublicStable),
        UpdateAction::Daemon(DaemonUpdateSource::ThisCli),
    ] {
        let error = run_update_action(action, Some(&executable)).unwrap_err();
        assert_eq!(
            error.to_string(),
            codex_install_context::EXTERNAL_UPDATE_MESSAGE
        );
    }
    assert!(!receipt.exists());
    Ok(())
}
