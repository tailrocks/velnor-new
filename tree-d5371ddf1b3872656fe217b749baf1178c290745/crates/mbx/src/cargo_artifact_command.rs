use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
use std::time::Instant;

pub struct CargoBoundStdout {
    pub(super) stream: ChildStdout,
    pub(super) binding: CargoCommandBinding,
}

pub struct CargoBoundStderr {
    pub(super) stream: ChildStderr,
    pub(super) binding: CargoCommandBinding,
}

/// Minted by the actual native spawn. Raw platform argument bytes are retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CargoCommandBinding {
    process_id: u32,
    session_id: String,
    root_session_id: String,
    program_bytes: Vec<u8>,
    #[serde(skip)]
    argument_bytes: Vec<Vec<u8>>,
    working_directory: PathBuf,
    command_sha256: String,
    #[serde(skip)]
    arguments: Vec<OsString>,
    #[serde(skip)]
    started_at: Instant,
}

impl CargoCommandBinding {
    #[cfg(test)]
    pub(super) fn fixture(arguments: &[&str]) -> Self {
        Self {
            process_id: 1,
            session_id: "test-session".into(),
            root_session_id: "test-root".into(),
            program_bytes: b"cargo".to_vec(),
            argument_bytes: Vec::new(),
            working_directory: PathBuf::from("/test"),
            command_sha256: "fixture".into(),
            arguments: arguments.iter().map(OsString::from).collect(),
            started_at: Instant::now(),
        }
    }
    pub fn spawn(
        command: &mut Command,
        session_id: String,
        root_session_id: String,
    ) -> std::io::Result<(Child, Self)> {
        let arguments: Vec<OsString> = command.get_args().map(OsString::from).collect();
        let working_directory = match command.get_current_dir() {
            Some(directory) => directory.to_path_buf(),
            None => std::env::current_dir()?,
        };
        let mut binding = Self {
            process_id: 0,
            session_id,
            root_session_id,
            program_bytes: command.get_program().as_encoded_bytes().to_vec(),
            argument_bytes: arguments
                .iter()
                .map(|argument| argument.as_encoded_bytes().to_vec())
                .collect(),
            working_directory,
            command_sha256: String::new(),
            arguments,
            started_at: Instant::now(),
        };
        let mut digest = Sha256::new();
        digest.update(b"mbx-native-cargo-command-v1\0");
        for bytes in std::iter::once(binding.program_bytes.as_slice())
            .chain(binding.argument_bytes.iter().map(Vec::as_slice))
        {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
        binding.command_sha256 = hex::encode(digest.finalize());
        binding.started_at = Instant::now();
        let child = command.spawn()?;
        binding.process_id = child.id();
        Ok((child, binding))
    }

    pub(super) fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
    pub(super) fn identity_matches(&self, session: &str, root: &str) -> bool {
        self.session_id == session && self.root_session_id == root
    }

    pub fn take_stdout(&self, child: &mut Child) -> std::io::Result<CargoBoundStdout> {
        self.check_child(child)?;
        let stream = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("Cargo stdout was not piped or already taken"))?;
        Ok(CargoBoundStdout {
            stream,
            binding: self.clone(),
        })
    }

    pub fn take_stderr(&self, child: &mut Child) -> std::io::Result<CargoBoundStderr> {
        self.check_child(child)?;
        let stream = child
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::other("Cargo stderr was not piped or already taken"))?;
        Ok(CargoBoundStderr {
            stream,
            binding: self.clone(),
        })
    }

    fn check_child(&self, child: &Child) -> std::io::Result<()> {
        if child.id() != self.process_id {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cargo stream child differs from bound spawn",
            ));
        }
        Ok(())
    }
}

/// Terminal process capability obtained only by waiting on the bound child.
pub struct CargoCommandCompletion {
    binding: CargoCommandBinding,
    status: ExitStatus,
    terminal_at: Instant,
}

impl CargoCommandCompletion {
    #[cfg(unix)]
    #[cfg(test)]
    pub(super) fn fixture(binding: CargoCommandBinding, success: bool) -> Self {
        use std::os::unix::process::ExitStatusExt;
        Self {
            binding,
            status: ExitStatus::from_raw(if success { 0 } else { 256 }),
            terminal_at: Instant::now(),
        }
    }
    pub fn wait(child: &mut Child, binding: CargoCommandBinding) -> std::io::Result<Self> {
        if child.id() != binding.process_id {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cargo completion child differs from bound spawn",
            ));
        }
        let status = child.wait()?;
        let terminal_at = Instant::now();
        Ok(Self {
            status,
            binding,
            terminal_at,
        })
    }

    pub fn status(&self) -> ExitStatus {
        self.status
    }
    pub fn workload_wall_ns(&self) -> u64 {
        u64::try_from(
            self.terminal_at
                .duration_since(self.binding.started_at)
                .as_nanos(),
        )
        .unwrap_or(u64::MAX)
    }
    pub(crate) fn started_at(&self) -> Instant {
        self.binding.started_at
    }
    pub(crate) fn terminal_at(&self) -> Instant {
        self.terminal_at
    }
    pub(super) fn binding(&self) -> &CargoCommandBinding {
        &self.binding
    }
}
