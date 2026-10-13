use std::path::Path;

use super::{bootout_argv, bootstrap_argv, with_logs};

#[test]
fn launchctl_uses_the_gui_domain_and_does_not_fork() {
    let plist = Path::new("/Users/example/Library/LaunchAgents/com.tailrocks.velnor.host.plist");
    assert_eq!(
        bootstrap_argv(501, plist),
        vec![
            "launchctl".to_owned(),
            "bootstrap".to_owned(),
            "gui/501".to_owned(),
            plist.display().to_string(),
        ]
    );
    assert_eq!(
        bootout_argv(501),
        vec![
            "launchctl".to_owned(),
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
    );
    assert!(body.contains("StandardOutPath"));
    assert!(body.contains("/Users/example/Library/Logs/Velnor/host.log"));
    assert!(!body.contains("ACTIONS_RUNNER_INPUT_JITCONFIG"));
    assert!(!body.contains("daemon fork"));
}
