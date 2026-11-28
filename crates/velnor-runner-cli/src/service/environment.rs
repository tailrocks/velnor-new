// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const ORB: &str = "orb";
const RUNNER: &str = "velnor-runner";
const ORB_KEY: &str = "VELNOR_ORB";
const RUNNER_KEY: &str = "VELNOR_RUNNER_BINARY";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Environment {
    pub(super) orb: PathBuf,
    pub(super) runner_binary: PathBuf,
}

pub(super) fn resolve(
    orb: Option<&Path>,
    runner_binary: Option<&Path>,
    host: &Path,
    search: Option<OsString>,
) -> Option<Environment> {
    let orb = match orb {
        Some(path) => executable(path).then(|| path.to_path_buf())?,
        None => std::env::split_paths(&search?)
            .map(|dir| dir.join(ORB))
            .find(|candidate| executable(candidate))?,
    };
    let runner_binary = match runner_binary {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(_) => return None,
        None => host.parent()?.join(RUNNER),
    };
    Some(Environment { orb, runner_binary })
}

pub(super) fn installed(plist: &Path) -> Option<Environment> {
    let text = std::fs::read_to_string(plist).ok()?;
    Some(Environment {
        orb: PathBuf::from(value(&text, ORB_KEY)?),
        runner_binary: PathBuf::from(value(&text, RUNNER_KEY)?),
    })
}

fn executable(path: &Path) -> bool {
    path.is_absolute()
        && std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn value(text: &str, key: &str) -> Option<String> {
    let open = format!("<key>{key}</key><string>");
    let (_, rest) = text.split_once(&open)?;
    let (raw, _) = rest.split_once("</string>")?;
    Some(
        raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Environment, installed};
    use crate::service::plist::render_plist;

    #[test]
    fn installed_environment_survives_a_rerender_for_start() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let environment = Environment {
            orb: root.join("a&b<c>").join("orb"),
            runner_binary: root.join("velnor-runner"),
        };
        let host = Path::new("/opt/velnor-host");
        let logs = root.join("logs");
        let body = render_plist(host, &environment, &root, &logs)
            .ok_or(std::io::ErrorKind::InvalidInput)?;
        let path = root.join("host.plist");
        std::fs::write(&path, &body)?;
        let read = installed(&path).ok_or(std::io::ErrorKind::InvalidData)?;
        assert_eq!(read, environment);
        assert_eq!(render_plist(host, &read, &root, &logs), Some(body));
        Ok(())
    }
}
