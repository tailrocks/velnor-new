//! Filesystem-only metadata for bounded Mise failure diagnostics.

use std::path::Path;

use super::push_line;

pub(super) fn append_path_metadata(report: &mut String, label: &str, path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            let target = if metadata.file_type().is_symlink() {
                append_symlink_target_metadata(report, label, path);
                std::fs::read_link(path).map_or_else(
                    |error| format!(" target_error={error}"),
                    |target| format!(" target={}", target.display()),
                )
            } else {
                String::new()
            };
            push_line(
                report,
                format_args!(
                    "{label}={} kind={kind} bytes={}{}",
                    path.display(),
                    metadata.len(),
                    target
                ),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            push_line(report, format_args!("{label}={} missing", path.display()));
        }
        Err(error) => {
            push_line(
                report,
                format_args!("{label}={} metadata_error={error}", path.display()),
            );
        }
    }
}

pub(super) fn append_tool_home_metadata(report: &mut String, key: &str, path: &Path) {
    match key {
        "CARGO_HOME" | "MISE_CARGO_HOME" => {
            for relative in ["bin", "bin/cargo", "bin/rustc", "bin/rustup"] {
                append_path_metadata(report, key, &path.join(relative));
            }
        }
        "RUSTUP_HOME" | "MISE_RUSTUP_HOME" => append_rustup_toolchain_metadata(report, key, path),
        _ => {}
    }
}

fn append_rustup_toolchain_metadata(report: &mut String, label: &str, rustup_home: &Path) {
    let toolchains = rustup_home.join("toolchains");
    append_path_metadata(report, label, &toolchains);
    let entries = match std::fs::read_dir(&toolchains) {
        Ok(entries) => entries,
        Err(error) => {
            push_line(
                report,
                format_args!("{label}-toolchains-list_error={error}"),
            );
            return;
        }
    };
    for entry in entries.take(64) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_line(
                    report,
                    format_args!("{label}-toolchains-entry_error={error}"),
                );
                continue;
            }
        };
        let name = entry.file_name();
        if !toolchain_matches_pin(&name.to_string_lossy()) {
            continue;
        }
        let toolchain = entry.path();
        append_path_metadata(report, label, &toolchain);
        for relative in ["bin/rustc", "bin/cargo", "lib/rustlib/components"] {
            append_path_metadata(report, label, &toolchain.join(relative));
        }
    }
}

fn toolchain_matches_pin(name: &str) -> bool {
    name == "1.99.1" || name.starts_with("1.99.1-")
}

fn append_symlink_target_metadata(report: &mut String, label: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let kind = metadata_kind(&metadata);
            push_line(
                report,
                format_args!(
                    "{label}-target={} exists kind={kind} bytes={}",
                    path.display(),
                    metadata.len()
                ),
            );
        }
        Err(error) => push_line(
            report,
            format_args!("{label}-target={} follow_error={error}", path.display()),
        ),
    }
    match path.canonicalize() {
        Ok(target) => push_line(
            report,
            format_args!(
                "{label}-target={} canonical={}",
                path.display(),
                target.display()
            ),
        ),
        Err(error) => push_line(
            report,
            format_args!(
                "{label}-target={} canonicalize_error={error}",
                path.display()
            ),
        ),
    }
}

fn metadata_kind(metadata: &std::fs::Metadata) -> &'static str {
    if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::{append_path_metadata, toolchain_matches_pin};

    #[test]
    fn pinned_rust_toolchain_match_is_closed_to_1991_names() {
        assert!(toolchain_matches_pin("1.99.1"));
        assert!(toolchain_matches_pin("1.99.1-aarch64-apple-darwin"));
        assert!(!toolchain_matches_pin("1.99.10-aarch64-apple-darwin"));
        assert!(!toolchain_matches_pin("stable-aarch64-apple-darwin"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_snapshot_reports_followed_target_metadata_and_failures() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "velnor-cli-mise-diagnostic-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("temporary metadata root");
        let target = root.join("cargo");
        std::fs::write(&target, b"cargo placeholder").expect("target file");
        let link = root.join("cargo-link");
        std::os::unix::fs::symlink(&target, &link).expect("target symlink");
        let dangling = root.join("dangling-link");
        std::os::unix::fs::symlink(root.join("absent"), &dangling).expect("dangling symlink");

        let mut report = String::new();
        append_path_metadata(&mut report, "cargo", &link);
        append_path_metadata(&mut report, "cargo", &dangling);

        assert!(report.contains("cargo-target=") && report.contains("exists kind=file"));
        assert!(report.contains("canonical="));
        assert!(report.contains("follow_error="));
        assert!(report.contains("canonicalize_error="));
        std::fs::remove_dir_all(root).expect("remove temporary metadata root");
    }
}
