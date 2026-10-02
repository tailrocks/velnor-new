//! A compiled, literal Cargo wrapper descriptor. No config or shell evaluation.
//! Hash verification requires the owner to prevent concurrent path mutation;
//! this is a dispatch boundary, not a filesystem sandbox or a sealed executable.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use eyre::{Result, bail, eyre};
use sha2::{Digest, Sha256};

const WRAPPER: &str = "MISE_OWNED_CARGO_WRAPPER";
const SHA256: &str = "MISE_OWNED_CARGO_WRAPPER_SHA256";

struct Descriptor {
    executable: PathBuf,
    sha256: String,
}

impl Descriptor {
    fn parse(executable: OsString, digest: OsString, flags: [Option<OsString>; 3]) -> Result<Self> {
        if flags
            .iter()
            .any(|flag| flag.as_deref() != Some(OsStr::new("1")))
        {
            bail!("requires MISE_NO_CONFIG=1, MISE_NO_ENV=1, and MISE_NO_HOOKS=1");
        }
        let executable = PathBuf::from(executable);
        if !executable.is_absolute() {
            bail!("executable must be an absolute literal path");
        }
        let sha256 = digest
            .into_string()
            .map_err(|_| eyre!("SHA256 must be UTF-8"))?;
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("SHA256 must contain exactly 64 lowercase hexadecimal digits");
        }
        Ok(Self { executable, sha256 })
    }

    fn verify(&self) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.executable)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("executable must be a regular file, never a symlink");
        }
        if std::fs::canonicalize(&self.executable)? != self.executable {
            bail!("executable path must be canonical");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o022 != 0 {
                bail!("executable must not be writable by group or others");
            }
            if metadata.permissions().mode() & 0o111 == 0 {
                bail!("executable has no execute permission");
            }
        }
        let mut file = std::fs::File::open(&self.executable)?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if hex::encode(hash.finalize()) != self.sha256 {
            bail!("executable SHA256 mismatch");
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn validate_shim(path: &Path, executable: &Path, allow_missing: bool) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| eyre!("shim has no parent directory"))?;
    match std::fs::symlink_metadata(directory) {
        Err(err) if allow_missing && err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            bail!("owned wrapper directory must be a real directory")
        }
        Ok(_) => {}
    }
    for entry in std::fs::read_dir(directory)? {
        if entry?.file_name() != "cargo" {
            bail!("owned wrapper directory may contain only the cargo shim");
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() && std::fs::read_link(path)? == executable =>
        {
            Ok(())
        }
        Err(err) if allow_missing && err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
        _ => bail!("owned cargo shim must link exactly to the current executable"),
    }
}

/// The only wrapper accepted by the compiled descriptor mode. This branch
/// precedes config and tool-derived wrappers, so no general payload is loaded.
pub(crate) fn wrapper() -> Result<Option<crate::config::CommandWrapper>> {
    let executable = std::env::var_os(WRAPPER);
    let digest = std::env::var_os(SHA256);
    let (executable, digest) = match (executable, digest) {
        (None, None) => return Ok(None),
        (Some(executable), Some(digest)) => (executable, digest),
        _ => bail!("both {WRAPPER} and {SHA256} are required"),
    };
    let descriptor = Descriptor::parse(
        executable,
        digest,
        [
            std::env::var_os("MISE_NO_CONFIG"),
            std::env::var_os("MISE_NO_ENV"),
            std::env::var_os("MISE_NO_HOOKS"),
        ],
    )?;
    descriptor.verify()?;
    let command = descriptor
        .executable
        .to_str()
        .ok_or_else(|| eyre!("wrapper path must be UTF-8"))?
        .to_owned();
    Ok(Some(crate::config::CommandWrapper::Detailed(
        crate::config::command_wrapper::CommandWrapperOptions {
            command,
            args: Vec::new(),
            env: [("MBX_CARGO_SHIM_MODE".into(), "1".into())].into(),
        },
    )))
}

pub fn dispatch() -> Result<Option<ExitCode>> {
    let executable = std::env::var_os(WRAPPER);
    let digest = std::env::var_os(SHA256);
    let (executable, digest) = match (executable, digest) {
        (None, None) => {
            let invoked = std::env::args_os()
                .next()
                .ok_or_else(|| eyre!("missing argv[0]"))?;
            let name = Path::new(&invoked)
                .file_name()
                .ok_or_else(|| eyre!("missing executable name"))?;
            if name == "cargo"
                || cfg!(windows) && name.to_string_lossy().eq_ignore_ascii_case("cargo.exe")
            {
                bail!("owned cargo shim requires both {WRAPPER} and {SHA256}");
            }
            return Ok(None);
        }
        (Some(executable), Some(digest)) => (executable, digest),
        _ => bail!("both {WRAPPER} and {SHA256} are required"),
    };
    let descriptor = Descriptor::parse(
        executable,
        digest,
        [
            std::env::var_os("MISE_NO_CONFIG"),
            std::env::var_os("MISE_NO_ENV"),
            std::env::var_os("MISE_NO_HOOKS"),
        ],
    )?;
    // Validate all invocations, including the outer `mise exec`, before any
    // startup fast path can load configuration or invoke an embedded tool.
    descriptor.verify()?;
    let mut args = std::env::args_os();
    let invoked = args.next().ok_or_else(|| eyre!("missing argv[0]"))?;
    let name = Path::new(&invoked)
        .file_name()
        .ok_or_else(|| eyre!("missing executable name"))?;
    if name != "cargo"
        && !(cfg!(windows) && name.to_string_lossy().eq_ignore_ascii_case("cargo.exe"))
    {
        if !crate::env::is_mise_binary(&name.to_string_lossy()) {
            bail!("owned wrapper mode supports only the cargo shim");
        }
        return Ok(None);
    }
    let mut command = Command::new(&descriptor.executable);
    command.args(args).env("MBX_CARGO_SHIM_MODE", "1");
    if let Some(path) = std::env::var_os("PATH") {
        let current = std::fs::canonicalize(std::env::current_exe()?)?;
        let filtered = std::env::split_paths(&path).filter(|dir| {
            !std::fs::canonicalize(dir.join(if cfg!(windows) { "cargo.exe" } else { "cargo" }))
                .is_ok_and(|candidate| candidate == current)
        });
        command.env("PATH", std::env::join_paths(filtered)?);
    }
    // Verify again immediately before native execution. Concurrent mutation of
    // an owner-controlled cache path remains outside this descriptor contract.
    descriptor.verify()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        let status = command.status()?;
        Ok(Some(ExitCode::from(
            u8::try_from(status.code().unwrap_or(1)).unwrap_or(1),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(path: &Path, digest: &str) -> Result<Descriptor> {
        Descriptor::parse(
            path.as_os_str().into(),
            digest.into(),
            [Some("1".into()), Some("1".into()), Some("1".into())],
        )
    }

    #[test]
    fn rejects_nonliteral_descriptor() {
        assert!(descriptor(Path::new("mbx"), &"0".repeat(64)).is_err());
        assert!(descriptor(Path::new("/mbx"), &"G".repeat(64)).is_err());
        assert!(descriptor(Path::new("/mbx"), &"0".repeat(63)).is_err());
        assert!(
            Descriptor::parse(
                "/mbx".into(),
                "0".repeat(64).into(),
                [Some("true".into()), Some("1".into()), Some("1".into())]
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_current_executable_can_own_cargo_shim() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cargo");
        let target = Path::new("/owned/mise");
        assert!(validate_shim(&path, target, true).is_ok());
        assert!(validate_shim(&path, target, false).is_err());
        std::fs::write(&path, "unowned executable").unwrap();
        assert!(validate_shim(&path, target, true).is_err());
        std::fs::remove_file(&path).unwrap();
        symlink("/ambient/mise", &path).unwrap();
        assert!(validate_shim(&path, target, true).is_err());
        std::fs::remove_file(&path).unwrap();
        symlink(target, &path).unwrap();
        assert!(validate_shim(&path, target, false).is_ok());
        std::fs::write(dir.path().join("rustc"), "unowned cache executable").unwrap();
        assert!(validate_shim(&path, target, false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validates_owner_bytes_and_rejects_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mbx");
        std::fs::write(&path, b"literal wrapper").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::fs::canonicalize(path).unwrap();
        let digest = hex::encode(Sha256::digest(b"literal wrapper"));
        let owner = descriptor(&path, &digest).unwrap();
        assert!(owner.verify().is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(owner.verify().is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&path, b"changed wrapper").unwrap();
        assert!(owner.verify().is_err());
        let link = path.with_extension("link");
        symlink(&path, &link).unwrap();
        assert!(descriptor(&link, &digest).unwrap().verify().is_err());
        assert!(
            descriptor(&path.with_extension("missing"), &digest)
                .unwrap()
                .verify()
                .is_err()
        );
    }
}
