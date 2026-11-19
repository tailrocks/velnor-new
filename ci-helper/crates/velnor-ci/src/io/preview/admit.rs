// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::cli::required;
use super::install::random_name;
use super::net::Transport;
use crate::error::CiError;
use crate::preview::admit::{Admission, admit};
use crate::preview::authority::modes::Inputs;
use crate::preview::json::{Res, canonical, require};
use crate::preview::login::Login;
use crate::preview::net::Net;

/// The whole job takes at most this long, as the legacy script's alarm made it.
const TOTAL: Duration = Duration::from_secs(120);
const METADATA: &str = "source-preview-metadata.json";

fn error(what: &str) -> CiError {
    CiError::new(format!("source preview admission: {what}"))
}

/// Create a file that must not exist. With `exact`, its mode is `mode` whatever the umask is.
fn create(path: &Path, mode: u32, exact: bool, data: &[u8]) -> Res<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| error("create"))?;
    if exact {
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| error("chmod"))?;
    }
    file.write_all(data).map_err(|_| error("write"))
}

/// Port of the end of `admit` and `main`: stage both payloads in a private directory,
/// hard-link them as `velnor-actions-<triple>` in `root` (never replacing a file), write
/// the metadata and append `source_sha` to `output`. A failure removes the links made.
pub fn publish(root: &Path, output: &Path, admission: &Admission, deadline: Instant) -> Res<()> {
    let staging = root.join(format!("sdk-preview-input-{}", random_name()?));
    DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(|_| error("staging"))?;
    let mut linked: Vec<PathBuf> = Vec::new();
    let result = (|| -> Res<()> {
        for (triple, data) in &admission.payloads {
            require(Instant::now() < deadline)?;
            let name = format!("velnor-actions-{triple}");
            create(&staging.join(&name), 0o500, true, data)?;
            fs::hard_link(staging.join(&name), root.join(&name)).map_err(|_| error("link"))?;
            linked.push(root.join(name));
        }
        fs::remove_dir_all(&staging).map_err(|_| error("staging"))?;
        require(Instant::now() < deadline)?;
        create(
            &root.join(METADATA),
            0o666,
            false,
            &canonical(&admission.metadata),
        )?;
        let source = admission.metadata["source_sha"].as_str().unwrap_or("");
        File::options()
            .append(true)
            .create(true)
            .open(output)
            .and_then(|mut f| f.write_all(format!("source_sha={source}\n").as_bytes()))
            .map_err(|_| error("output"))
    })();
    if result.is_err() {
        drop(fs::remove_dir_all(&staging));
        for path in &linked {
            drop(fs::remove_file(path));
        }
    }
    result
}

/// `velnor-ci admit-preview`.
pub fn admit_preview() -> Res<String> {
    let vars: BTreeMap<String, String> = std::env::vars().collect();
    let root = PathBuf::from(required(&vars, "RUNNER_TEMP")?);
    let output = PathBuf::from(required(&vars, "GITHUB_OUTPUT")?);
    // An unusable login leaves the account path closed; the admission then refuses it.
    let actor = vars
        .get("GITHUB_TRIGGERING_ACTOR")
        .and_then(|login| Login::parse(login).ok());
    let net =
        Transport::with_total(required(&vars, "GH_TOKEN")?.to_owned(), TOTAL).with_actor(actor);
    let admission =
        admit(&net, &Inputs { vars }).map_err(|e| e.context("source preview admission failed"))?;
    publish(&root, &output, &admission, net.deadline())
        .map_err(|e| e.context("source preview admission failed"))?;
    Ok(String::new())
}
