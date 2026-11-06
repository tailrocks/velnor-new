// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::ExitCode;

use velnor_runner_host::{DeleteDecision, HostConfig, connect_unix, remove_recorded};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(journal), Some(intent)) = (args.next(), args.next()) else {
        eprintln!("usage: stage_once <journal-path> <intent-id>");
        return ExitCode::from(2);
    };
    let Ok(intent_id) = intent.parse::<i64>() else {
        return ExitCode::from(2);
    };
    let Some(home) = std::env::var_os("HOME") else {
        return ExitCode::from(1);
    };
    let path = PathBuf::from(home).join("Library/Application Support/Velnor/host.toml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return ExitCode::from(1);
    };
    let Ok(config) = HostConfig::parse(&text) else {
        return ExitCode::from(1);
    };
    let Ok(docker) = connect_unix(&config.docker.endpoint) else {
        return ExitCode::from(1);
    };
    match remove_recorded(&docker, &PathBuf::from(journal), intent_id).await {
        Ok(decision) => {
            let decision = match decision {
                DeleteDecision::Delete => "deleted",
                DeleteDecision::KeepForeign => "held",
                DeleteDecision::NotDeleted => "already-clean",
            };
            println!("decision={decision} intent_id={intent_id}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("stage_once: {error}");
            ExitCode::from(1)
        }
    }
}
