use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn invalid(label: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("private_git_repository:{label}:{detail}"),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn with_path(error: &io::Error, label: &str, path: &Path) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("private_git_repository:{label}:{}:{error}", path.display()),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "private_git_repository:unsupported_platform",
    )
}
