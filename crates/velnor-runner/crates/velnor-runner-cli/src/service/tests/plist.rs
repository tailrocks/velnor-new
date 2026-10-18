#[test]
fn launchctl_uses_the_gui_domain_and_does_not_fork() {
    let plist = Path::new("/Users/example/Library/LaunchAgents/com.tailrocks.velnor.host.plist");
    assert_eq!(
        bootstrap_argv(501, plist),
        vec![
            "/bin/launchctl".to_owned(),
            "bootstrap".to_owned(),
            "gui/501".to_owned(),
            plist.display().to_string(),
        ]
    );
    assert_eq!(
        bootout_argv(501),
        vec![
            "/bin/launchctl".to_owned(),
            "bootout".to_owned(),
            "gui/501/com.tailrocks.velnor.host".to_owned(),
        ]
    );
}

#[test]
fn plist_logs_stay_out_of_the_secret_channel() {
    let body = with_logs(
        "<dict>\n</dict>\n",
        Path::new("/Users/example/Library/Logs/Velnor"),
    )
    .unwrap_or_default();
    assert!(body.contains("StandardOutPath"));
    assert!(body.contains("/Users/example/Library/Logs/Velnor/host.log"));
    assert!(!body.contains("ACTIONS_RUNNER_INPUT_JITCONFIG"));
    assert!(!body.contains("daemon fork"));
}

#[test]
fn plist_runs_the_daemon_with_the_global_state_path() -> Result<(), String> {
    let plist = velnor_runner_host::launch_agent_plist(Path::new("/opt/velnor-host"))
        .map_err(|error| error.to_string())?;
    let body = with_state(&plist, Path::new("/tmp/velnor&<state>"))
        .ok_or_else(|| "state argument missing".to_owned())?;
    assert!(body.contains(
        "<string>/opt/velnor-host</string>\n<string>--state</string><string>/tmp/velnor&amp;&lt;state&gt;</string><string>daemon</string>"
    ));
    Ok(())
}

#[test]
fn plist_rejects_a_state_path_that_xml_cannot_preserve() -> Result<(), String> {
    let plist = velnor_runner_host::launch_agent_plist(Path::new("/opt/velnor-host"))
        .map_err(|error| error.to_string())?;
    assert!(with_state(&plist, Path::new("/tmp/velnor\nstate")).is_none());
    assert!(with_state(&plist, Path::new("/tmp/velnor\u{0001}state")).is_none());
    assert!(with_logs("<dict></dict>", Path::new("/tmp/velnor\u{0001}logs")).is_none());
    Ok(())
}
