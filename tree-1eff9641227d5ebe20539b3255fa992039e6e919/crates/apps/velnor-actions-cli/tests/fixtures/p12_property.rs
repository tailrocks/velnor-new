//! P12-8 risk-triggered property tests on a deterministic std-only harness.
//!
//! No property library is pinned yet (wanted: proptest 1.11.0, reported
//! to the parent), so these properties run on a small seeded xorshift
//! generator through the public binary surface: arbitrary configs and
//! repo layouts must never panic, and plan/generate outputs must be
//! deterministic functions of their inputs. A failing case prints its
//! seed and index so the exact input reproduces.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Output;

use crate::impl_cli_tmp as tmp;

/// Deterministic xorshift64* generator; fixed seeds, no external crates.
struct Rng {
    /// Current state; never zero so the stream never sticks.
    state: u64,
}

impl Rng {
    /// Generator over the stream selected by `seed`.
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    /// Next output word.
    fn next(&mut self) -> u64 {
        let mut word = self.state;
        word ^= word >> 12;
        word ^= word << 25;
        word ^= word >> 27;
        self.state = word;
        word.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Value in `0..bound` (`bound` must be nonzero).
    fn below(&mut self, bound: usize) -> usize {
        let span = u64::try_from(bound).unwrap_or(u64::MAX);
        usize::try_from(self.next() % span).unwrap_or(0)
    }

    /// `len` arbitrary bytes.
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            out.extend_from_slice(&self.next().to_le_bytes());
        }
        out.truncate(len);
        out
    }

    /// One of `choices`, chosen uniformly.
    fn pick<'a>(&mut self, choices: &[&'a str]) -> &'a str {
        choices[self.below(choices.len())]
    }
}

/// TOML/config-shaped fragments, hostile spellings included.
const CONFIG_FRAGMENTS: [&str; 44] = [
    "[workflow]",
    "[stacks]",
    "[stacks.rust]",
    "[discovery]",
    "[actions.overrides]",
    "[[actions]]",
    "schema = 1",
    "policy = ",
    "default_branch = ",
    "compile_driver = ",
    "test_runner = ",
    "ignore = ",
    "exclude = ",
    "\"main\"",
    "\"rust\"",
    "\"mbx\"",
    "\"cargo_nextest\"",
    "true",
    "123",
    "=",
    "#",
    "\n",
    "\r\n",
    "\t",
    " ",
    "[",
    "]",
    "{",
    "}",
    "\"",
    "'",
    "\\",
    "..",
    "/",
    ":",
    "-",
    "héllo",
    "🦀",
    "\u{1}",
    "\u{7f}",
    "x",
    "A",
    "0",
    ".",
];

/// Filename characters: legal everywhere plus hostile-but-writable ones.
const NAME_FRAGMENTS: [&str; 24] = [
    "a", "Z", "0", "-", "_", ".", " ", "é", "\n", "\t", "'", "\"", "#", "%", "&", ";", "$", "`",
    "!", "(", ")", "[", "]", "~",
];

/// Lowercase crate-name alphabet (cargo-valid spellings only).
const CRATE_CHARS: [char; 38] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-', '_',
];

/// Random config body from `CONFIG_FRAGMENTS`.
fn gen_config(rng: &mut Rng) -> String {
    let parts = rng.below(40);
    let mut body = String::new();
    for _ in 0..parts {
        body.push_str(rng.pick(&CONFIG_FRAGMENTS));
    }
    body
}

/// Random hostile filename (never empty, never `.`/`..`, no separator).
fn gen_filename(rng: &mut Rng) -> String {
    let parts = 1 + rng.below(6);
    let mut name = String::new();
    for _ in 0..parts {
        name.push_str(rng.pick(&NAME_FRAGMENTS));
    }
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return format!("f{}", rng.below(10_000));
    }
    name
}

/// Random cargo-valid crate name.
fn gen_crate_name(rng: &mut Rng) -> String {
    let len = 1 + rng.below(11);
    let mut name = String::new();
    name.push(CRATE_CHARS[rng.below(26)]);
    for _ in 1..len {
        name.push(CRATE_CHARS[rng.below(CRATE_CHARS.len())]);
    }
    name
}

/// Assert a child exited gracefully: no signal death, no panic signature.
fn assert_graceful(case: &str, output: &Output) {
    let code = tmp::code(output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(code != -1, "{case}: died by signal");
    assert!(code != 101, "{case}: panicked (exit 101):\n{stderr}");
    assert!(
        !stderr.contains("panicked"),
        "{case}: panic text:\n{stderr}"
    );
    assert!(!stdout.contains("panicked"), "{case}: panic on stdout");
}

/// `Workspace crates: N` count from plan output.
fn workspace_crates(stdout: &str) -> Result<usize, Box<dyn Error>> {
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("Workspace crates: ") {
            return Ok(rest.trim().parse::<usize>()?);
        }
    }
    Err("plan lacks a Workspace crates line".into())
}

/// Every regular file's bytes under `dir`: relative path plus content.
type Snapshot = Vec<(PathBuf, Vec<u8>)>;

/// Every regular file's bytes under `dir`, sorted by relative path.
fn snapshot(dir: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            entries.push(entry?.path());
        }
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                pending.push(entry);
            } else {
                let rel = entry.strip_prefix(dir)?.to_path_buf();
                out.push((rel, std::fs::read(&entry)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Initialized repo whose config is `body` plus hostile sibling files.
fn garbage_repo(prefix: &str, rng: &mut Rng, body: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    let repo = tmp::fresh_tempdir(prefix)?;
    tmp::init_repo(&repo)?;
    std::fs::write(repo.join(".velnor/config.toml"), body)?;
    for _ in 0..rng.below(4) {
        let name = gen_filename(rng);
        let len = rng.below(200);
        std::fs::write(repo.join(name), rng.bytes(len))?;
    }
    Ok(repo)
}

/// Initialized repo holding `count` valid generated crates.
fn crate_repo(
    prefix: &str,
    rng: &mut Rng,
    count: usize,
) -> Result<(PathBuf, Vec<String>), Box<dyn Error>> {
    let repo = tmp::fresh_tempdir(prefix)?;
    tmp::init_repo(&repo)?;
    let mut names = Vec::new();
    for _ in 0..count {
        let mut name = gen_crate_name(rng);
        for _ in 0..100 {
            if !names.contains(&name) {
                break;
            }
            name = gen_crate_name(rng);
        }
        if names.contains(&name) {
            return Err("crate name stream exhausted".into());
        }
        let dir = repo.join("crates").join(&name);
        std::fs::create_dir_all(dir.join("src"))?;
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        std::fs::write(dir.join("src/lib.rs"), "")?;
        names.push(name);
    }
    Ok((repo, names))
}

#[test]
fn property_path_documented_honestly() -> Result<(), Box<dyn Error>> {
    let procedure = super::read("docs/implemented/update-procedure.md")?;
    for marker in [
        "p12_property",
        "proptest",
        "1.11.0",
        "Fuzzing is infeasible",
        "merge entrypoint",
    ] {
        assert!(procedure.contains(marker), "procedure misses {marker}");
    }
    assert!(
        !procedure.contains("proptest ="),
        "procedure must not claim a proptest pin that is not wired"
    );
    Ok(())
}

#[test]
fn property_plan_never_panics_on_arbitrary_config() -> Result<(), Box<dyn Error>> {
    let mut rng = Rng::new(0x5043_4647_3132_3801);
    for index in 0..96 {
        let body = gen_config(&mut rng);
        let repo = garbage_repo("p12-prop-cfg", &mut rng, body.as_bytes())?;
        let output = tmp::spawn(&["plan"], &[], &repo)?;
        assert_graceful(&format!("seed cfg case {index}: {body:?}"), &output);
        tmp::cleanup(&repo);
    }
    Ok(())
}

#[test]
fn property_plan_never_panics_on_arbitrary_bytes() -> Result<(), Box<dyn Error>> {
    let mut rng = Rng::new(0x5043_4647_3132_3802);
    for index in 0..64 {
        let len = rng.below(300);
        let body = rng.bytes(len);
        let repo = garbage_repo("p12-prop-bytes", &mut rng, &body)?;
        let output = tmp::spawn(&["plan"], &[], &repo)?;
        assert_graceful(&format!("seed bytes case {index}: {body:?}"), &output);
        tmp::cleanup(&repo);
    }
    Ok(())
}

#[test]
fn property_plan_count_exact_and_deterministic() -> Result<(), Box<dyn Error>> {
    let mut rng = Rng::new(0x5043_4647_3132_3803);
    for index in 0..24 {
        let count = rng.below(4);
        let (repo, names) = crate_repo("p12-prop-count", &mut rng, count)?;
        let first = tmp::spawn(&["plan"], &[], &repo)?;
        assert_eq!(
            tmp::code(&first),
            0,
            "case {index} names {names:?}: {:?}",
            String::from_utf8_lossy(&first.stderr)
        );
        let stdout = String::from_utf8(first.stdout.clone())?;
        assert_eq!(
            workspace_crates(&stdout)?,
            names.len(),
            "case {index} names {names:?}"
        );
        let second = tmp::spawn(&["plan"], &[], &repo)?;
        assert_eq!(tmp::code(&second), 0, "case {index} rerun failed");
        assert_eq!(
            second.stdout, first.stdout,
            "case {index} plan not deterministic"
        );
        tmp::cleanup(&repo);
    }
    Ok(())
}

#[test]
fn property_generate_repeats_byte_identical() -> Result<(), Box<dyn Error>> {
    let mut rng = Rng::new(0x5043_4647_3132_3804);
    for index in 0..8 {
        let count = rng.below(3);
        let (repo, names) = crate_repo("p12-prop-gen", &mut rng, count)?;
        let plan = tmp::spawn(&["plan"], &[], &repo)?;
        assert_eq!(tmp::code(&plan), 0, "case {index} plan failed");
        let first = tmp::fresh_tempdir("p12-prop-gen-a")?;
        let second = tmp::fresh_tempdir("p12-prop-gen-b")?;
        for dir in [&first, &second] {
            let output = tmp::spawn(
                &["generate", "--output-dir", dir.to_str().unwrap_or("/")],
                &[],
                &repo,
            )?;
            assert_eq!(
                tmp::code(&output),
                0,
                "case {index} names {names:?} generate failed: {:?}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            snapshot(&first)?,
            snapshot(&second)?,
            "case {index} names {names:?} generate not deterministic"
        );
        assert!(
            first.join(".github/workflows").is_dir(),
            "case {index} lacks a workflow tree"
        );
        tmp::cleanup(&repo);
        tmp::cleanup(&first);
        tmp::cleanup(&second);
    }
    Ok(())
}
