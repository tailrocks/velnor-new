use std::fs;
use std::path::{Path, PathBuf};

use velnor_gen::{expected_files, expected_with_notes};
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::{CONFIG_PATH, LOCK_PATH};

use crate::golden::{load, sample_dirs, samples};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, private_repo};
use crate::release_fixture::{APP, LOCK, release_view};

const TEST_ONLY_FLAGS: [&str; 3] = ["--no-fixed-toolchain", "--trusted-dir", "--home"];

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn leaks<'a>(texts: impl IntoIterator<Item = (String, &'a str)>) -> Vec<String> {
    texts
        .into_iter()
        .filter(|(_, text)| TEST_ONLY_FLAGS.iter().any(|flag| text.contains(flag)))
        .map(|(origin, _)| origin)
        .collect()
}

fn files_below(root: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

#[test]
fn the_scan_finds_each_flag_in_a_rendered_file() {
    for flag in TEST_ONLY_FLAGS {
        let text = format!("run: fetch --manifest-path Cargo.toml {flag} x\n");
        let found = leaks([(".github/workflows/rust-ci.yml".to_owned(), text.as_str())]);
        assert_eq!(found, [".github/workflows/rust-ci.yml"], "{flag}");
    }
}

#[test]
fn no_rendered_workflow_carries_the_test_only_fetch_flags() -> TestResult {
    let mut found = Vec::new();
    for sample in sample_dirs(&samples())? {
        let (files, _) = expected_with_notes(&load(&sample)?)?;
        found.extend(leaks(files.iter().map(|(path, text)| {
            (
                format!("{} renders {path}", sample.display()),
                text.as_str(),
            )
        })));
    }
    assert!(found.is_empty(), "{found:?}");
    Ok(())
}

#[test]
fn no_private_sources_workflow_carries_the_test_only_fetch_flags() -> TestResult {
    let mut found = Vec::new();
    let mut fetched = false;
    for shape in ["one-job", "lint-test", "groups"] {
        let repo = private_repo(
            &["Cargo.toml"],
            &format!("shape = '{shape}'\n{SECTION}"),
            "0.1.88",
        )?;
        let text = workflow(&repo)?;
        fetched |= text.contains("fetch --manifest-path");
        found.extend(leaks([(
            format!("{shape} renders rust-ci.yml"),
            text.as_str(),
        )]));
    }
    let binary = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n";
    let mut release = release_view(&[APP], Some(binary))?;
    release.files.insert(
        CONFIG_PATH,
        format!("full_run = \"off\"\n{SECTION}").into_bytes(),
    );
    release
        .files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", "0.1.88").into_bytes());
    let files = expected_files(&release)?;
    let text = files.get(&RELEASE_WORKFLOW_PATH).ok_or("no release.yml")?;
    assert!(
        text.contains("fetch --manifest-path"),
        "no private release fetch"
    );
    assert!(fetched, "no shape rendered a private fetch step");
    found.extend(leaks(files.iter().map(|(path, text)| {
        (format!("release renders {path}"), text.as_str())
    })));
    assert!(found.is_empty(), "{found:?}");
    Ok(())
}

#[test]
fn no_checked_in_sample_or_workflow_carries_the_test_only_fetch_flags() -> TestResult {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = files_below(&samples())?;
    paths.extend(files_below(&repository.join(".github/workflows"))?);
    assert!(!paths.is_empty(), "nothing to scan");
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        found.extend(leaks([(path.display().to_string(), text.as_str())]));
    }
    assert!(found.is_empty(), "{found:?}");
    Ok(())
}
