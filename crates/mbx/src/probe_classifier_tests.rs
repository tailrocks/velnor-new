use super::rustc_probe;
use std::ffi::OsString;

fn probe(values: &[&str]) -> bool {
    rustc_probe(&values.iter().map(OsString::from).collect::<Vec<_>>())
}

#[test]
fn version_and_help_require_entire_query_shape() {
    for values in [
        vec!["--version"],
        vec!["-vV"],
        vec!["-Vv"],
        vec!["--version", "--verbose"],
        vec!["--help", "-v"],
    ] {
        assert!(probe(&values), "{values:?}");
    }
    for values in [
        vec![],
        vec!["-v"],
        vec!["src.rs", "--version"],
        vec!["src.rs", "--out-dir", "--version"],
        vec!["src.rs", "-L", "--version"],
        vec!["src.rs", "--out-dir=--version"],
        vec!["src.rs", "-L--version"],
        vec!["--unknown", "--version"],
    ] {
        assert!(!probe(&values), "{values:?}");
    }
}

#[test]
fn print_queries_consume_values_and_preserve_work() {
    assert!(probe(&[
        "-",
        "--crate-name",
        "___",
        "--crate-type=bin",
        "--print=file-names",
        "--print",
        "cfg",
    ]));
    for flag in ["--out-dir", "--crate-name", "--target", "-L", "-C", "-W"] {
        for value in ["--version", "--print=cfg"] {
            assert!(!probe(&["src.rs", flag, value]), "{flag} {value}");
            assert!(probe(&[flag, value, "--print=cfg"]), "{flag} {value}");
        }
    }
    for values in [
        vec!["src.rs", "--unknown", "--print=cfg"],
        vec!["src.rs", "--out-dir=--print=cfg"],
        vec!["src.rs", "-Clink-arg=--print=cfg"],
        vec!["src.rs", "--print=link-args"],
        vec!["src.rs", "--print=native-static-libs"],
        vec!["-", "--crate-name", "stdin_crate", "--emit=link"],
        vec!["--print=cfg", "--emit=link"],
        vec!["--print=cfg", "--crate-name"],
        vec!["--print=cfg", "--crate-name="],
        vec!["--print"],
        vec!["--print=unknown"],
        vec!["--print=cfg", "src.rs", "other.rs"],
        vec!["@response", "--print=cfg"],
        vec!["--out-dir", "@response", "--print=cfg"],
    ] {
        assert!(!probe(&values), "{values:?}");
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_options_or_values_cannot_prove_probe() {
    use std::os::unix::ffi::OsStringExt;
    assert!(!rustc_probe(&[
        "--out-dir".into(),
        OsString::from_vec(vec![0xff]),
        "--print=cfg".into(),
    ]));
}
