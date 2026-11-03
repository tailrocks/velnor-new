// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::NonInputs;

use super::non_inputs_grammar::{base_config, load};
use super::scoped_workspace::{TestResult, put};
use super::workflow_configuration_fixture::{git, git_fixture};

const CORPUS: [&str; 49] = [
    "docs/a.md",
    "docs/guide.md",
    "docs/sub/b.md",
    "docs/sub/deep/c.txt",
    "docs/docs/r.md",
    "docs/sub/docs/x.md",
    "docs/sub/sub/y.md",
    "sub/docs/z.md",
    "sub/a.md",
    "sub/sub/s.md",
    "sub/sub/sub/t.md",
    "a-b/q.md",
    "a-b/docs/q.md",
    "a-b/sub/q.md",
    "a-b/docs/sub/q.md",
    "docs/a-b/u.md",
    "docs/sub/a-b/v.md",
    "a/b/c/d/e.md",
    "docsx/c.md",
    "docs2/a.md",
    "docs-old/a.md",
    "src/docs/a.md",
    "apps/a1/docs/d.md",
    ".docs/z.md",
    "docs/.hidden",
    "docs/sub/.hid/x",
    ".cargo/config.toml",
    "docs/with space.md",
    "with space/a.md",
    "docs/dir with space/f.md",
    "docs/\u{e9}.md",
    "docs/\u{fc}n\u{ef}/a.md",
    "pics/\u{e9}.png",
    "pics/a.png",
    "pics/ab.png",
    "pics/\u{e9}/x.png",
    "pics/a/x.png",
    "\u{65e5}\u{672c}/a.md",
    "docs/[x].md",
    "docs/{a,b}.md",
    "docs/!bang.md",
    "docs/a?b.md",
    "docs/a*b.md",
    "docs/#h.md",
    "docs/-dash.md",
    "docs/new\nline.md",
    "README.md",
    "a.b",
    "file.v1.md",
];

const MAX_REPORTED: usize = 5;

const ALPHABET: [&str; 15] = [
    "docs", "Docs", "sub", "a-b", "*", "?", "[a]", "\u{e9}", "", ".", "..", "do*", "{a,b}", "a b",
    ".cargo",
];

const NARROW: [&str; 3] = ["docs", "sub", "*"];

const PREFIXES: [&str; 4] = ["", "./", "/", "**/"];

const TAILS: [&str; 6] = ["/**", "", "/", "/*", "/**/*.md", "/*.md"];

const MUST_ACCEPT: [&str; 6] = [
    "docs/**",
    "docs/sub/**",
    "docs/*/**",
    "*/docs/**",
    "a-b/docs/**",
    "Docs/**",
];

const MUST_REJECT: [&str; 11] = [
    ".cargo",
    "docs",
    "shared",
    "[]a]pps/**",
    "pics/?.png",
    "docs/*",
    "**/docs/**",
    "./x/**",
    "x//y/**",
    "docs/{a,b}/**",
    "\u{e9}/**",
];

pub(super) fn git_text(root: &Path, args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

fn paths_of(bytes: &[u8]) -> BTreeSet<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect()
}

pub(super) fn tracked(root: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    Ok(paths_of(&git_text(root, &["ls-files", "-z"])?))
}

pub(super) fn sparse_set(
    origin: &Path,
    non_inputs: &NonInputs,
    ignorecase: bool,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let scratch = tempfile::tempdir()?;
    let checkout = scratch.path().join("co");
    let status = git_fixture::sterile_command()
        .args(["clone", "-q", "--no-checkout"])
        .arg(origin)
        .arg(&checkout)
        .status()?;
    assert!(status.success(), "clone of {} failed", origin.display());
    git(
        &["config", "core.ignorecase", &ignorecase.to_string()],
        &checkout,
    )?;
    git(&["config", "core.sparseCheckout", "true"], &checkout)?;
    let info = String::from_utf8(git_text(
        &checkout,
        &["rev-parse", "--git-path", "info/sparse-checkout"],
    )?)?;
    let info = checkout.join(info.trim());
    fs::create_dir_all(info.parent().ok_or("no parent")?)?;
    fs::write(
        &info,
        format!("\n{}\n", non_inputs.sparse_checkout_patterns().join("\n")),
    )?;
    git(&["read-tree", "-mu", "HEAD"], &checkout)?;
    let listing = git_text(&checkout, &["ls-files", "-t", "-z"])?;
    Ok(listing
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(b"S "))
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

fn guard_set(origin: &Path, non_inputs: &NonInputs) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let output = git_fixture::command(origin)?
        .args(["-c", "core.ignorecase=false", "ls-files", "-z", "--"])
        .args(non_inputs.pathspecs())
        .output()?;
    assert!(output.status.success());
    Ok(paths_of(&output.stdout))
}

pub(super) fn planner_set(corpus: &BTreeSet<String>, non_inputs: &NonInputs) -> BTreeSet<String> {
    corpus
        .iter()
        .filter(|path| non_inputs.contains(path))
        .cloned()
        .collect()
}

fn corpus_repo() -> Result<TempDir, Box<dyn Error>> {
    let repo = tempfile::tempdir()?;
    git(&["init", "-b", "testmain"], repo.path())?;
    for path in CORPUS {
        put(repo.path(), path, "x\n")?;
    }
    git(&["add", "-A"], repo.path())?;
    git(&["commit", "-m", "corpus"], repo.path())?;
    Ok(repo)
}

fn candidates() -> BTreeSet<String> {
    let mut sequences: Vec<Vec<&str>> = ALPHABET.iter().map(|seg| vec![*seg]).collect();
    for first in ALPHABET {
        for second in ALPHABET {
            sequences.push(vec![first, second]);
        }
    }
    for first in NARROW {
        for second in NARROW {
            for third in NARROW {
                sequences.push(vec![first, second, third]);
            }
        }
    }
    let mut all = BTreeSet::new();
    for prefix in PREFIXES {
        for sequence in &sequences {
            for tail in TAILS {
                all.insert(format!("{prefix}{}{tail}", sequence.join("/")));
            }
        }
    }
    all.extend(MUST_ACCEPT.iter().map(|glob| (*glob).to_owned()));
    all.extend(MUST_REJECT.iter().map(|glob| (*glob).to_owned()));
    all
}

fn difference(left: &BTreeSet<String>, right: &BTreeSet<String>) -> BTreeSet<String> {
    left.symmetric_difference(right).cloned().collect()
}

#[test]
fn accepted_non_input_globs_classify_identically_in_planner_sparse_checkout_and_guard() -> TestResult
{
    let repo = corpus_repo()?;
    let origin = repo.path();
    let corpus = tracked(origin)?;
    assert_eq!(corpus.len(), CORPUS.len());
    let base = base_config()?;
    let mut accepted: Vec<(String, BTreeSet<String>)> = Vec::new();
    let mut rejected = BTreeSet::new();
    let mut failures = Vec::new();
    for glob in candidates() {
        if failures.len() >= MAX_REPORTED {
            break;
        }
        match load(&base, &[&glob])? {
            Err(_) => {
                rejected.insert(glob);
            }
            Ok(non_inputs) => {
                let planner = planner_set(&corpus, &non_inputs);
                let sparse = sparse_set(origin, &non_inputs, false)?;
                let guard = guard_set(origin, &non_inputs)?;
                if planner != sparse || sparse != guard {
                    failures.push(format!(
                        "{glob}: planner vs sparse {:?}, sparse vs guard {:?}, planner vs guard {:?}",
                        difference(&planner, &sparse),
                        difference(&sparse, &guard),
                        difference(&planner, &guard)
                    ));
                }
                accepted.push((glob, planner));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    let accepted_globs: BTreeSet<&str> = accepted.iter().map(|(glob, _)| glob.as_str()).collect();
    for glob in MUST_ACCEPT {
        assert!(accepted_globs.contains(glob), "{glob} must be accepted");
    }
    for glob in MUST_REJECT {
        assert!(rejected.contains(glob), "{glob} must be rejected");
    }
    assert!(accepted.iter().any(|(_, set)| !set.is_empty()));
    assert!(accepted.iter().any(|(_, set)| set.is_empty()));
    let star = load(&base, &["docs/*/**"])?.map_err(|error| error.to_string())?;
    for set in [
        planner_set(&corpus, &star),
        sparse_set(origin, &star, false)?,
        guard_set(origin, &star)?,
    ] {
        assert!(!set.contains("docs/guide.md"), "{set:?}");
        assert!(set.contains("docs/sub/b.md"), "{set:?}");
    }
    Ok(())
}
