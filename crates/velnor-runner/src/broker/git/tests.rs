// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::git;

fn alive(pid: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .expect("run ps");
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

async fn pid_from(file: &Path) -> String {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(file)
            && !text.trim().is_empty()
        {
            return text.trim().to_owned();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("git alias never wrote its pid");
}

#[tokio::test]
async fn dropping_a_running_git_on_a_current_thread_runtime_kills_its_group() {
    let dir = std::env::temp_dir().join(format!("velnor-git-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let file = dir.join("pid");
    let alias = format!("alias.s=!echo $$ > {}; exec sleep 30", file.display());
    let args = ["-c".to_owned(), alias, "s".to_owned()];
    let run = tokio::time::timeout(Duration::from_secs(2), git(&args, &[]));
    let (outcome, pid) = tokio::join!(run, pid_from(&file));
    let survived = alive(&pid);
    if survived {
        let _ = Command::new("kill").args(["-KILL", &pid]).status();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(outcome.is_err(), "git should still have been running");
    assert!(
        !survived,
        "the git process group outlived the dropped future"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_holding_stderr_dies_when_the_leader_already_exited() {
    let dir = std::env::temp_dir().join(format!("velnor-git-member-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let file = dir.join("pid");
    let alias = format!("alias.s=!sleep 30 & echo $! > {}; exit 0", file.display());
    let args = ["-c".to_owned(), alias, "s".to_owned()];
    let run = tokio::time::timeout(Duration::from_secs(2), git(&args, &[]));
    let (outcome, pid) = tokio::join!(run, pid_from(&file));
    let survived = alive(&pid);
    if survived {
        let _ = Command::new("kill").args(["-KILL", &pid]).status();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(outcome.is_err(), "the member holds stderr open");
    assert!(!survived, "the group member outlived the dropped future");
}
