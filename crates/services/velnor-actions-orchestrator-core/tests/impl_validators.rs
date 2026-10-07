//! I/O hardening cases: git arg validators.

use std::ffi::OsString;

use velnor_actions_orchestrator_core::{
    GitArgError, validate_diff_args, validate_diff_rev, validate_git_args, validate_rev,
    validate_select_diff_args, validate_select_show_args, validate_show_args, validate_show_path,
};

/// Full 40-hex revision fixture.
fn sha(byte: u8) -> String {
    let digit = format!("{byte:02x}");
    digit.repeat(20)
}

/// `OsString` arguments from string parts.
fn args(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

/// `git diff` shape from the `changed_files` call path, with full SHAs.
fn diff_shape(base: &str, head: &str) -> Vec<OsString> {
    args(&["--name-only", &format!("{base}...{head}"), "--"])
}

/// `git show` shape from the `batch_manifests` call path, with full SHAs.
fn show_shape(base: &str, manifests: &[&str]) -> Vec<OsString> {
    let mut parts = vec![
        "-s".to_owned(),
        "--format=%x00velnor-base-manifest%x00".to_owned(),
    ];
    for (index, manifest) in manifests.iter().enumerate() {
        if index > 0 {
            parts.push(base.to_owned());
        }
        parts.push(format!("{base}:{manifest}"));
    }
    parts.push("--".to_owned());
    parts.iter().map(OsString::from).collect()
}

#[test]
fn git_rev_accepts_full_sha_only() {
    assert!(validate_rev(&sha(0xab)).is_ok());
    for bad in [
        "",
        "HEAD",
        "main",
        "-n",
        "--upload-pack=x",
        &sha(0xab)[..7],
        &sha(0xab)[..39],
        &format!("{}0", sha(0xab)),
        &"z".repeat(40),
        "https://example.invalid/x.git",
    ] {
        assert!(
            validate_rev(bad).is_err(),
            "revision must fail closed: {bad}"
        );
    }
}

#[test]
fn git_diff_accepts_selection_call_shapes() {
    let base = sha(0x11);
    let head = sha(0x22);
    assert!(validate_diff_args(&diff_shape(&base, &head)).is_ok());
    let added = args(&[
        "--name-only",
        "--no-renames",
        "--diff-filter=A",
        &format!("{base}...{head}"),
        "--",
    ]);
    assert!(validate_diff_args(&added).is_ok());
    assert!(validate_diff_args(&args(&[&base])).is_ok());
}

#[test]
fn git_diff_accepts_deleted_call_shape() {
    let base = sha(0x11);
    let head = sha(0x22);
    let deleted = args(&[
        "--name-only",
        "--no-renames",
        "--diff-filter=D",
        &format!("{base}...{head}"),
        "--",
    ]);
    assert!(validate_select_diff_args(&deleted).is_ok());
    let lower = args(&[
        "--name-only",
        "--no-renames",
        "--diff-filter=d",
        &format!("{base}...{head}"),
        "--",
    ]);
    assert!(validate_select_diff_args(&lower).is_err());
}

#[test]
fn git_diff_rejects_injection() {
    let base = sha(0x11);
    let head = sha(0x22);
    let range = format!("{base}...{head}");
    let cases: Vec<Vec<OsString>> = vec![
        args(&["--name-only", "--upload-pack=evil", &range, "--"]),
        args(&["--name-only", "-c", &range, "--"]),
        args(&["--name-only", "HEAD", "--"]),
        args(&["--name-only", &format!("{base}...HEAD"), "--"]),
        args(&["--name-only", &format!("{}...{}", &base[..7], head), "--"]),
        args(&["--name-only", &range, "../escape"]),
        args(&["--name-only", &range, "/abs/path"]),
        args(&["--name-only", &range, "--", "--output=/tmp/x"]),
        args(&["--diff-filter", "A", &range]),
    ];
    for case in cases {
        assert!(
            validate_diff_args(&case).is_err(),
            "diff must fail closed: {case:?}"
        );
    }
}

#[test]
fn git_show_accepts_batch_call_shape() {
    let base = sha(0x33);
    let call = show_shape(&base, &["Cargo.toml", "crates/demo/Cargo.toml"]);
    assert!(validate_show_args(&call).is_ok());
    assert!(validate_show_path("crates/demo/Cargo.toml").is_ok());
}

#[test]
fn git_show_rejects_injection() {
    let base = sha(0x33);
    let flag = "--format=%x00velnor-base-manifest%x00";
    let spec = format!("{base}:Cargo.toml");
    let cases: Vec<Vec<OsString>> = vec![
        args(&["-s", flag, "--upload-pack=evil", "--"]),
        args(&["-s", "--format=--upload-pack=evil", &spec, "--"]),
        args(&["-s", "--format=%H %s", &spec, "--"]),
        args(&["-s", "--format=", &spec, "--"]),
        args(&["-s", flag, "HEAD:Cargo.toml", "--"]),
        args(&["-s", flag, &format!("{base}:../escape"), "--"]),
        args(&["-s", flag, &format!("{base}:/etc/passwd"), "--"]),
        args(&["-s", flag, &format!("{base}:-x"), "--"]),
        args(&["-s", flag, &format!("{base}:a:b"), "--"]),
        args(&["-s", flag, &format!("{base}:a\nb"), "--"]),
        args(&["-s", flag, "HEAD", "--"]),
        args(&["--output=/tmp/x", flag, &spec, "--"]),
    ];
    for case in cases {
        assert!(
            validate_show_args(&case).is_err(),
            "show must fail closed: {case:?}"
        );
    }
    assert!(matches!(
        validate_show_args(&args(&["-s", "HEAD:Cargo.toml"])),
        Err(GitArgError::BadRevision { .. })
    ));
    assert!(matches!(
        validate_show_path("../escape"),
        Err(GitArgError::BadPath { .. })
    ));
}

#[test]
fn git_args_reject_non_utf8_and_other_verbs() {
    assert!(matches!(
        validate_git_args("diff", &diff_shape(&sha(0x11), &sha(0x22))),
        Ok(())
    ));
    assert!(matches!(
        validate_git_args("show", &show_shape(&sha(0x11), &["Cargo.toml"])),
        Ok(())
    ));
    assert!(validate_git_args("upload-pack", &[]).is_err());
    assert!(validate_git_args("rev-parse", &args(&["--show-toplevel"])).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let non_utf8 = OsString::from_vec(vec![0xff, 0xfe]);
        assert!(matches!(
            validate_diff_args(&[non_utf8]),
            Err(GitArgError::NonUtf8Arg)
        ));
    }
}

#[test]
fn git_diff_rev_accepts_short_sha_for_selection() {
    let full = sha(0xab);
    assert!(validate_diff_rev(&full, "bad_base").is_ok());
    assert!(validate_diff_rev(&full[..7], "bad_base").is_ok());
    assert!(validate_diff_rev(&full[..4], "bad_head").is_ok());
    assert_eq!(
        validate_diff_rev("HEAD", "bad_base"),
        Err("bad_base_must_be_hex_sha".to_owned())
    );
    assert_eq!(
        validate_diff_rev("-x", "bad_head"),
        Err("bad_head_leading_dash".to_owned())
    );
    assert!(validate_diff_rev(&full[..3], "bad_base").is_err());
    assert!(validate_diff_rev(&format!("{full}0"), "bad_base").is_err());
}

#[test]
fn git_select_args_accept_short_sha_and_reject_injection() {
    let base = sha(0x11);
    let head = sha(0x22);
    let short = base[..7].to_owned();
    assert!(validate_select_diff_args(&diff_shape(&short, &head)).is_ok());
    assert!(validate_select_show_args(&show_shape(&short, &["Cargo.toml"])).is_ok());
    assert!(validate_select_diff_args(&diff_shape(&base, &head)).is_ok());
    assert!(validate_select_show_args(&show_shape(&base, &["Cargo.toml"])).is_ok());
    let range = format!("{short}...{head}");
    let flag = "--format=%x00velnor-base-manifest%x00";
    for case in [
        args(&["--name-only", "--upload-pack=evil", &range, "--"]),
        args(&["--name-only", "HEAD", "--"]),
        args(&["--name-only", &format!("{short}...HEAD"), "--"]),
    ] {
        assert!(
            validate_select_diff_args(&case).is_err(),
            "diff rejects: {case:?}"
        );
    }
    for case in [
        args(&["-s", flag, "--upload-pack=evil", "--"]),
        args(&["-s", flag, "HEAD:Cargo.toml", "--"]),
        args(&["-s", flag, &format!("{short}:../escape"), "--"]),
    ] {
        assert!(
            validate_select_show_args(&case).is_err(),
            "show rejects: {case:?}"
        );
    }
}
