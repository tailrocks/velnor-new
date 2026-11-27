// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::TcpListener;
use std::time::Duration;

use super::{Broker, Request, Response, TrustedFacts, Window};

fn facts(server: &str) -> TrustedFacts {
    TrustedFacts::parse(
        format!(
            r#"{{"repository_id": 1, "repository": "tailrocks/velnor-new",
"server_url": "{server}", "default_branch": "main", "sha": "{sha}", "run_id": 1,
"run_attempt": 1, "event_name": "push", "event": {{"kind": "branch", "branch": "main"}}}}"#,
            sha = "a".repeat(40)
        )
        .as_bytes(),
    )
    .expect("facts")
}

#[tokio::test]
async fn a_repair_that_outlives_its_cap_reports_failure_and_stops_git() {
    let silent = TcpListener::bind("127.0.0.1:0").expect("listen");
    let server = format!(
        "http://127.0.0.1:{}",
        silent.local_addr().expect("addr").port()
    );
    let root = std::env::temp_dir().join(format!("velnor-cap-{}", std::process::id()));
    let broker = Broker::new(facts(&server), root.clone()).with_repair_cap(Duration::from_secs(1));
    let request = Request::RepairMirror {
        token: None,
        window: Window::COMMIT,
    };
    let response = tokio::time::timeout(Duration::from_secs(60), broker.handle(request)).await;
    let _ = std::fs::remove_dir_all(&root);
    match response {
        Ok(Response::Failed { message }) => {
            assert!(message.contains("longer than 1 seconds"), "{message}");
        }
        other => panic!("the cap did not end the repair: {other:?}"),
    }
}
