use super::super::*;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::SocketAddr;

fn entry(path: String, kind: RuntimeEntryKind) -> RuntimeEntryEvidence {
    RuntimeEntryEvidence {
        path,
        kind,
        owner: 501,
    }
}

fn maximum_path_bytes() -> usize {
    (1..256)
        .take_while(|length| {
            SocketAddr::from_pathname(format!("/{}", "x".repeat(length - 1))).is_ok()
        })
        .last()
        .expect("Unix path capacity")
}

#[test]
fn socket_paths_follow_target_abi_boundary_including_nul() {
    let maximum = maximum_path_bytes();
    #[cfg(target_os = "macos")]
    assert_eq!(maximum + 1, 104);
    #[cfg(target_os = "linux")]
    assert_eq!(maximum + 1, 108);
    let home = Path::new("/h");
    let prefix = home.join(".orbstack/run").as_os_str().as_bytes().len() + 1;
    let fitting = "s".repeat(maximum - prefix);
    validate(
        home,
        &[entry(fitting.clone(), RuntimeEntryKind::UnixSocket)],
    )
    .expect("maximum path plus NUL fits");
    let error = validate(
        home,
        &[entry(format!("{fitting}s"), RuntimeEntryKind::UnixSocket)],
    )
    .expect_err("one byte over capacity");
    assert!(error.to_string().contains("runtime_socket_path_too_long"));
    assert!(error.to_string().contains("shorten RUNNER_TEMP"));
}

#[test]
fn socket_paths_count_utf8_bytes_and_all_nested_sockets() {
    let maximum = maximum_path_bytes();
    let home = Path::new("/h");
    let prefix = home.join(".orbstack/run").as_os_str().as_bytes().len() + 1;
    let nested = format!("sub/{}", "é".repeat((maximum - prefix - 4) / 2 + 1));
    let entries = [
        entry("docker.sock".to_owned(), RuntimeEntryKind::UnixSocket),
        entry(nested, RuntimeEntryKind::UnixSocket),
    ];
    assert!(validate(home, &entries).is_err());
    assert!(validate(Path::new("/"), &entries).is_ok());
}

#[test]
fn long_directory_and_regular_file_entries_do_not_consume_socket_budget() {
    let entries = [
        entry("d".repeat(300), RuntimeEntryKind::Directory),
        entry("f".repeat(300), RuntimeEntryKind::RegularFile),
        entry("docker.sock".to_owned(), RuntimeEntryKind::UnixSocket),
    ];
    validate(Path::new("/h"), &entries).expect("only sockets consume Unix address budget");
}

#[test]
fn socket_error_escapes_control_bytes_without_raw_terminal_output() {
    let home = Path::new("/h\n\u{1b}");
    let error = validate(
        home,
        &[entry("s".repeat(300), RuntimeEntryKind::UnixSocket)],
    )
    .expect_err("overlong control-byte path");
    let message = error.to_string();
    assert!(message.contains("/h\\n\\x1b/"));
    assert!(!message.contains('\n'));
    assert!(!message.contains('\u{1b}'));
}
