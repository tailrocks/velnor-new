// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_local::io::authority::Backend;
use velnor_local::io::supervisor::{self, Pairs};

use crate::support::temp_dir;

struct Never;

impl Backend for Never {
    fn get(
        &self,
        _: velnor_local::side::Side,
        _: &str,
    ) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        std::future::ready(Ok(None))
    }

    fn delete(
        &self,
        _: velnor_local::side::Side,
        _: &str,
    ) -> impl Future<Output = io::Result<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        _: velnor_local::side::Side,
        _: Option<&str>,
    ) -> impl Future<
        Output = Result<Option<velnor_local::io::pair::Minted>, velnor_local::io::pair::MintError>,
    > + Send {
        std::future::ready(Ok(None))
    }
}

async fn arguments_given_to_supervise(name: &str, local_ci: Option<&str>) -> Vec<String> {
    let dir = temp_dir(name);
    let script = dir.join("recording-authority");
    std::fs::write(
        &script,
        "#!/bin/bash\nmkdir -p \"$3/runner\"\nprintf '%s\\n' \"$@\" > \"$3/recorded\"\necho 127.0.0.1:9 > \"$3/authority/address\"\necho 'pair trusted-1 listening' > \"$3/runner/velnor-host.log\"\nexec /bin/sleep 600\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pairs = Pairs {
        host: PathBuf::from("/host"),
        trusted: 1,
        untrusted: 0,
        policy: None,
        local_ci: local_ci.map(PathBuf::from),
        ..Pairs::default()
    };
    let started = supervisor::start_pairs(
        &dir,
        &script,
        &pairs,
        async |_: &Path| -> io::Result<Never> { Err(io::Error::other("stop here")) },
        Duration::from_secs(5),
    )
    .await;
    assert!(started.is_err());
    std::fs::read_to_string(dir.join("recorded"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn supervise_is_given_the_velnor_ci_path_only_when_one_was_asked_for() {
    let without = arguments_given_to_supervise("pairs-local-ci-off", None).await;
    assert!(
        without.iter().all(|arg| arg != "--velnor-ci-binary"),
        "{without:?}"
    );
    let with = arguments_given_to_supervise("pairs-local-ci-on", Some("/x/velnor-ci")).await;
    let at = with
        .iter()
        .position(|arg| arg == "--velnor-ci-binary")
        .unwrap_or_else(|| unreachable!("{with:?}"));
    assert_eq!(with[at + 1], "/x/velnor-ci");
}
