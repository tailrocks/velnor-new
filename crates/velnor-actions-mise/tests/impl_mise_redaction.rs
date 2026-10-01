//! Secret redaction: `Debug` never carries credential material.
//!
//! [`ReleaseRequest`] already redacts its token; these cases prove the
//! redaction survives the typed chain into [`PinnedToolExec`] and
//! [`IsolatedCommand`], that secret-looking env values redact while
//! ordinary values stay visible, and that `CARGO_REGISTRY_TOKEN` is a
//! reserved key (a repo task carrying it would silently disable OIDC).
use std::ffi::OsString;
use std::path::PathBuf;

use velnor_actions_mise::catalog::release_plz::ReleaseRequest;
use velnor_actions_mise::command::is_reserved_env_key;
use velnor_actions_mise::{IsolatedCommand, PinnedTool, PinnedToolExec, ToolCatalog};

fn tokened_argv() -> Vec<OsString> {
    ["release", "--config", "r.toml", "--token", "sekrit-value"]
        .iter()
        .map(OsString::from)
        .collect()
}

#[test]
fn pinned_exec_debug_redacts_token_value() {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::ReleasePlz],
        std::ffi::OsStr::new("release-plz"),
        tokened_argv(),
    )
    .expect("exec");
    let debug = format!("{exec:?}");
    assert!(!debug.contains("sekrit-value"), "token leaked: {debug}");
    assert!(debug.contains("--token"), "flag shape kept: {debug}");
    assert!(debug.contains("<redacted>"), "marker kept: {debug}");
}

#[test]
fn isolated_command_debug_redacts_token_value() {
    let command = ReleaseRequest::release_with_token(PathBuf::from("r.toml"), "sekrit-value")
        .expect("tokened")
        .command(&ToolCatalog::pinned())
        .expect("command");
    let debug = format!("{command:?}");
    assert!(!debug.contains("sekrit-value"), "token leaked: {debug}");
    assert!(debug.contains("--token"), "flag shape kept: {debug}");
    assert!(debug.contains("<redacted>"), "marker kept: {debug}");
    assert!(debug.contains("mise"), "program kept: {debug}");
}

#[test]
fn oidc_command_debug_has_no_token_shape() {
    let command = ReleaseRequest::release(PathBuf::from("r.toml"))
        .expect("oidc")
        .command(&ToolCatalog::pinned())
        .expect("command");
    let debug = format!("{command:?}");
    assert!(!debug.contains("--token"), "no token flag: {debug}");
    assert!(!debug.contains("<redacted>"), "no marker: {debug}");
}

#[test]
fn secret_env_redacts_while_plain_values_stay_visible() {
    let specs = ToolCatalog::pinned().tool_specs(&[PinnedTool::Rust]);
    let payload = [OsString::from("cargo"), OsString::from("--version")];
    let command = IsolatedCommand::mise_exec(&specs, &payload)
        .expect("exec")
        .with_env(&[
            (OsString::from("MY_API_KEY"), OsString::from("sekrit-value")),
            (OsString::from("VELNOR_TASK_RUN"), OsString::from("ok")),
        ]);
    let debug = format!("{command:?}");
    assert!(!debug.contains("sekrit-value"), "secret leaked: {debug}");
    assert!(debug.contains("MY_API_KEY"), "name kept: {debug}");
    assert!(debug.contains("<redacted>"), "marker kept: {debug}");
    assert!(debug.contains("ok"), "plain value kept: {debug}");
}

#[test]
fn registry_token_is_reserved() {
    assert!(is_reserved_env_key("CARGO_REGISTRY_TOKEN"));
    let declared = vec![(
        OsString::from("CARGO_REGISTRY_TOKEN"),
        OsString::from("sekrit-value"),
    )];
    assert!(
        IsolatedCommand::repo_task("sh", Vec::new(), &declared).is_err(),
        "repo tasks must reject the registry token"
    );
    let specs = ToolCatalog::pinned().tool_specs(&[PinnedTool::Rust]);
    let payload = [OsString::from("cargo"), OsString::from("--version")];
    let command = IsolatedCommand::mise_exec(&specs, &payload).expect("exec");
    assert!(
        command.with_env(&declared).is_err(),
        "extras must reject the registry token"
    );
}
