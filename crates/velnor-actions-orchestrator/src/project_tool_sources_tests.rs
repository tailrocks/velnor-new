use std::fs;

use velnor_actions_mise::PinnedTool;

use super::{declared_versions, parse_source, read_nested_toolchains, read_project_sources};

fn values(path: &str, text: &str) -> Vec<(PinnedTool, String, String)> {
    parse_source(path, text)
        .expect("parse")
        .into_iter()
        .map(|d| (d.tool, d.key, d.value))
        .collect()
}

#[test]
fn mise_toml_records_every_spelling_and_shape() {
    let text = "[tools]\nrust = \"1.99.1\"\n\"core:rust\" = \"1.99.1\"\nnode = \"20\"\ngh = \"2.0.0\"\nshellcheck = [\"1.0.0\"]\nopentofu = { version = 3 }\nactionlint = { os = [\"linux\"] }\n\"aqua:nextest-rs/nextest/cargo-nextest\" = { version = \"0.9.148\" }\n";
    let got = values("mise.toml", text);
    let rendered: Vec<_> = got
        .iter()
        .map(|(tool, key, value)| format!("{}|{key}|{value}", tool.tool_name()))
        .collect();
    assert_eq!(
        rendered,
        [
            "nextest|aqua:nextest-rs/nextest/cargo-nextest|0.9.148",
            "rust|core:rust|1.99.1",
            "opentofu|opentofu|3",
            "rust|rust|1.99.1",
            "shellcheck|shellcheck|[\"1.0.0\"]",
        ]
    );
}

#[test]
fn tool_versions_and_toolchain_files_parse_like_the_resolver() {
    assert_eq!(
        values(
            ".tool-versions",
            "# c\nrust 1.99.1 # note\njava 21\nshellcheck 1.0.0 1.0.1\n"
        ),
        [
            (PinnedTool::Rust, "rust".to_owned(), "1.99.1".to_owned()),
            (
                PinnedTool::Shellcheck,
                "shellcheck".to_owned(),
                "1.0.0 1.0.1".to_owned()
            ),
        ]
    );
    for (path, text) in [
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
        ("rust-toolchain", "[toolchain]\nchannel = \"1.99.1\"\n"),
        ("rust-toolchain", " 1.99.1 \r\n"),
    ] {
        assert_eq!(
            values(path, text),
            [(
                PinnedTool::Rust,
                "toolchain.channel".to_owned(),
                "1.99.1".to_owned()
            )],
            "{path}"
        );
    }
    assert!(parse_source("rust-toolchain.toml", "[toolchain\n").is_err());
    assert!(parse_source("mise.toml", "[tools\n").is_err());
    assert!(declared_versions("mise.local.toml", "x =").is_err());
}

#[test]
fn every_mise_location_is_read_and_a_symlink_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for path in [
        "mise.toml",
        ".config/mise.toml",
        ".config/mise/conf.d/a.toml",
        "mise/config.toml",
        ".mise/config.toml",
        "mise.ci.toml",
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, "[tools]\nrust = \"1.99.1\"\n").expect("write");
    }
    fs::write(root.join(".tool-versions"), "shellcheck 0.11.0\n").expect("write");
    let files: Vec<String> = read_project_sources(root)
        .expect("read")
        .into_iter()
        .map(|source| source.file)
        .collect();
    assert_eq!(
        files,
        [
            ".config/mise.toml",
            ".config/mise/conf.d/a.toml",
            ".mise/config.toml",
            "mise.ci.toml",
            "mise.toml",
            "mise/config.toml",
            ".tool-versions",
        ]
    );
    std::os::unix::fs::symlink("mise.toml", root.join("mise.local.toml")).expect("link");
    assert!(read_project_sources(root).is_err());
}

#[test]
fn nested_toolchains_are_found_without_git_and_skip_build_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for (path, text) in [
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
        (
            "sub/ws/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.98.0\"\n",
        ),
        ("sub/legacy/rust-toolchain", "1.97.1\n"),
        (
            "target/x/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.0.0\"\n",
        ),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }
    let found: Vec<_> = read_nested_toolchains(root)
        .expect("nested")
        .into_iter()
        .map(|source| (source.file, source.declarations[0].value.clone()))
        .collect();
    assert_eq!(
        found,
        [
            ("sub/legacy/rust-toolchain".to_owned(), "1.97.1".to_owned()),
            ("sub/ws/rust-toolchain.toml".to_owned(), "1.98.0".to_owned()),
        ]
    );
}
