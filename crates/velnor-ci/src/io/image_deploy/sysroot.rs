// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::{Body, Https, Request, Transport, read_limited};
use crate::cli_image::ImageSysrootArgs;
use crate::error::CiError;
use crate::image_deploy::lock::{
    ARCH_HEADERS, Deb, HEADERS, INCLUDE_DIRECTORY, LIB_DIRECTORY, parse_lock, verify,
};

const MAX_LOCK_BYTES: u64 = 1 << 20;
const MAX_DEB_BYTES: u64 = 1 << 28;
const MAX_REDIRECTS: usize = 3;
const ENVIRONMENT: &str = "GITHUB_ENV";

fn failure(action: &str, path: &Path, error: &std::io::Error) -> CiError {
    CiError::new(format!("cannot {action} {}: {error}", path.display()))
}

fn download(transport: &dyn Transport, url: &str) -> Result<Vec<u8>, CiError> {
    let mut url = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        let response = transport.send(Request {
            method: "GET",
            url: url.clone(),
            headers: Vec::new(),
            body: Body::Empty,
        })?;
        match response.status {
            200 => return response.bounded(MAX_DEB_BYTES),
            301 | 302 | 303 | 307 | 308 => {
                url = redirect_target(&url, response.header("location")).ok_or_else(|| {
                    CiError::new("a download redirected somewhere other than https")
                })?;
            }
            status => {
                return Err(CiError::new(format!(
                    "a download of the sysroot answered HTTP {status}"
                )));
            }
        }
    }
    Err(CiError::new(
        "a download of the sysroot redirected too often",
    ))
}

fn redirect_target(current: &str, location: Option<&str>) -> Option<String> {
    let location = location?;
    if location.starts_with("https://") {
        return Some(location.to_owned());
    }
    let origin_end = current
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/'))
        .map(|slash| slash + "https://".len())?;
    (location.starts_with('/') && !location.starts_with("//"))
        .then(|| format!("{}{location}", &current[..origin_end]))
}

const MARKER: &str = ".velnor-image-sysroot";

fn prepare_out(out: &Path) -> Result<(), CiError> {
    if out.exists() {
        let ours = out.join(MARKER).is_file();
        let empty = fs::read_dir(out).is_ok_and(|mut entries| entries.next().is_none());
        if !ours && !empty {
            return Err(CiError::new(format!(
                "{} exists and is not an earlier sysroot of this command; choose another --out",
                out.display()
            )));
        }
        fs::remove_dir_all(out).map_err(|error| failure("replace", out, &error))?;
    }
    fs::create_dir_all(out).map_err(|error| failure("create", out, &error))?;
    fs::write(out.join(MARKER), "").map_err(|error| failure("write", &out.join(MARKER), &error))
}

fn extract(dpkg_deb: &Path, deb: &Path, root: &Path) -> Result<(), CiError> {
    let status = Command::new(dpkg_deb)
        .env_clear()
        .arg("--extract")
        .arg(deb)
        .arg(root)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| failure("run", dpkg_deb, &error))?;
    if status.success() {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "{} could not unpack {}",
            dpkg_deb.display(),
            deb.display()
        )))
    }
}

fn merge_headers(root: &Path) -> Result<(), CiError> {
    let from = root.join(ARCH_HEADERS);
    let to = root.join(HEADERS);
    let Ok(entries) = fs::read_dir(&from) else {
        return Ok(());
    };
    fs::create_dir_all(&to).map_err(|error| failure("create", &to, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| failure("read", &from, &error))?;
        let target = to.join(entry.file_name());
        fs::copy(entry.path(), &target).map_err(|error| failure("copy to", &target, &error))?;
    }
    Ok(())
}

fn drop_static_libraries(lib: &Path) -> Result<(), CiError> {
    for entry in fs::read_dir(lib).map_err(|error| failure("read", lib, &error))? {
        let path = entry.map_err(|error| failure("read", lib, &error))?.path();
        if path.extension().is_some_and(|extension| extension == "a") {
            fs::remove_file(&path).map_err(|error| failure("remove", &path, &error))?;
        }
    }
    Ok(())
}

fn fetch_all(
    debs: &[Deb],
    transport: &dyn Transport,
    dpkg_deb: &Path,
    out: &Path,
) -> Result<PathBuf, CiError> {
    let root = out.join("root");
    let downloads = out.join("debs");
    for directory in [&root, &downloads] {
        fs::create_dir_all(directory).map_err(|error| failure("create", directory, &error))?;
    }
    for (index, deb) in debs.iter().enumerate() {
        let bytes = download(transport, &deb.url)?;
        verify(deb, &bytes)?;
        let file = downloads.join(format!("{index}-{}.deb", deb.package));
        fs::write(&file, &bytes).map_err(|error| failure("write", &file, &error))?;
        extract(dpkg_deb, &file, &root)?;
    }
    Ok(root)
}

pub(super) fn unpack(args: &ImageSysrootArgs) -> Result<String, CiError> {
    let environment = std::env::var_os(ENVIRONMENT).map(PathBuf::from);
    unpack_with(args, &Https::new(), environment.as_deref())
}

pub fn unpack_with(
    args: &ImageSysrootArgs,
    transport: &dyn Transport,
    environment: Option<&Path>,
) -> Result<String, CiError> {
    let lock = fs::File::open(&args.lock)
        .map_err(|error| failure("read", &args.lock, &error))
        .and_then(|file| read_limited(file, MAX_LOCK_BYTES))?;
    let debs = parse_lock(
        std::str::from_utf8(&lock).map_err(|_| CiError::new("the sysroot lock is not UTF-8"))?,
    )?;
    let out = args.out.as_path();
    prepare_out(out)?;
    let root = fetch_all(&debs, transport, &args.dpkg_deb, out)?;
    let lib = root.join(LIB_DIRECTORY);
    merge_headers(&root)?;
    drop_static_libraries(&lib)
        .map_err(|_| CiError::new("the packages hold no OpenSSL libraries"))?;
    for library in ["libssl.so", "libcrypto.so"] {
        if fs::metadata(lib.join(library)).is_err() {
            return Err(CiError::new(format!(
                "the packages of the sysroot lock do not provide {library}"
            )));
        }
    }
    if let Some(file) = environment {
        let lines = format!(
            "X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR={}\nX86_64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR={}\n",
            lib.display(),
            root.join(INCLUDE_DIRECTORY).display()
        );
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(file)
            .and_then(|mut handle| handle.write_all(lines.as_bytes()))
            .map_err(|error| failure("write", file, &error))?;
    }
    Ok(format!(
        "unpacked {} packages into {}\n",
        debs.len(),
        out.display()
    ))
}
