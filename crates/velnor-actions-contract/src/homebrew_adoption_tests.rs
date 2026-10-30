use super::*;

#[path = "homebrew_adoption_seed_v2_tests.rs"]
mod seed_v2;

fn locator() -> HomebrewSourceAuthorityLocator {
    HomebrewSourceAuthorityLocator {
        schema: crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1.to_owned(),
        repository: "example/project".to_owned(),
        repository_id: 42,
        default_branch: "main".to_owned(),
        config_path: crate::homebrew_authority::HOMEBREW_SOURCE_CONFIG_PATH.to_owned(),
        release_policy_path: crate::homebrew_authority::HOMEBREW_SOURCE_RELEASE_POLICY_PATH
            .to_owned(),
    }
}

fn identity(path: &str, mode: u32, bytes: &[u8]) -> HomebrewAdoptionFileIdentity {
    HomebrewAdoptionFileIdentity {
        path: path.to_owned(),
        mode,
        blob_sha: fake_blob_sha(bytes),
    }
}

fn fake_blob_sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write as _;
    let mut output = String::with_capacity(40);
    for byte in sha2::Sha256::digest(bytes).iter().take(20) {
        write!(&mut output, "{byte:02x}").expect("string write");
    }
    output
}

type AdoptionFixture = (
    HomebrewAdoptionTransition,
    Vec<HomebrewAdoptionTreeFile>,
    Vec<HomebrewAdoptionTreeFile>,
    Vec<u8>,
    BTreeMap<String, Vec<u8>>,
    BTreeMap<String, Vec<u8>>,
);

fn transition(base: &BTreeMap<String, (u32, Vec<u8>)>) -> HomebrewAdoptionTransition {
    let deleted_paths = [
        "scripts/a.py",
        "scripts/b.py",
        "scripts/c.py",
        "scripts/d.py",
        "scripts/e.py",
        "scripts/f.py",
        "scripts/g.py",
        "scripts/h.py",
        "scripts/i.py",
    ];
    let deletions = deleted_paths
        .iter()
        .map(|path| {
            let (mode, bytes) = base.get(*path).expect("deleted fixture file");
            identity(path, *mode, bytes)
        })
        .collect();
    let inert_paths = [
        "Formula/jackin-dev.rb",
        "Formula/jackin-preview.rb",
        "Formula/jackin.rb",
        "lib/jackin_formula_support.rb",
    ];
    let unchanged_inert_files = inert_paths
        .iter()
        .map(|path| {
            let (mode, bytes) = base.get(*path).expect("inert fixture file");
            identity(path, *mode, bytes)
        })
        .collect();
    HomebrewAdoptionTransition {
        schema: HOMEBREW_ADOPTION_TRANSITION_SCHEMA_V1.to_owned(),
        transition_id: "initial-seed".to_owned(),
        tap_repository: "example/tap".to_owned(),
        tap_repository_id: 43,
        base_sha: "a".repeat(40),
        base_tree: "b".repeat(40),
        template_version: 2,
        minimum_sdk_version: "0.1.55".to_owned(),
        required_sdk_capability: "homebrew-adoption-seed-v1".to_owned(),
        source_authority_locator: locator(),
        claude_source_path: "AGENTS.md".to_owned(),
        claude_source_blob_sha: fake_blob_sha(b"policy\n"),
        deletions,
        unchanged_inert_files,
        generated_output_paths: vec![
            ".github/workflows/ci.yml".to_owned(),
            ".velnor/config.toml".to_owned(),
            ".velnor/release-manifest.json".to_owned(),
            "mise.lock".to_owned(),
            "mise.toml".to_owned(),
        ],
        required_operations: vec![
            NativeHomebrewOperation::TapTree,
            NativeHomebrewOperation::AdoptionContract,
        ],
    }
}

fn base_inventory() -> BTreeMap<String, (u32, Vec<u8>)> {
    let mut base = BTreeMap::from([
        ("AGENTS.md".to_owned(), (0o100_644, b"policy\n".to_vec())),
        ("CLAUDE.md".to_owned(), (0o120_000, b"AGENTS.md".to_vec())),
        ("README.md".to_owned(), (0o100_644, b"readme\n".to_vec())),
        (
            ".github/workflows/ci.yml".to_owned(),
            (0o100_644, b"old workflow\n".to_vec()),
        ),
        (
            ".velnor/config.toml".to_owned(),
            (0o100_644, b"old config\n".to_vec()),
        ),
        (
            ".velnor/release-manifest.json".to_owned(),
            (0o100_644, br#"{"schema":1,"version":"0.1.48"}"#.to_vec()),
        ),
        ("mise.lock".to_owned(), (0o100_644, b"old lock\n".to_vec())),
        ("mise.toml".to_owned(), (0o100_644, b"old mise\n".to_vec())),
        (
            "Formula/jackin-dev.rb".to_owned(),
            (0o100_644, b"dev\n".to_vec()),
        ),
        (
            "Formula/jackin-preview.rb".to_owned(),
            (0o100_644, b"preview\n".to_vec()),
        ),
        (
            "Formula/jackin.rb".to_owned(),
            (0o100_644, b"stable\n".to_vec()),
        ),
        (
            "lib/jackin_formula_support.rb".to_owned(),
            (0o100_644, b"helper\n".to_vec()),
        ),
    ]);
    for (index, path) in [
        "scripts/a.py",
        "scripts/b.py",
        "scripts/c.py",
        "scripts/d.py",
        "scripts/e.py",
        "scripts/f.py",
        "scripts/g.py",
        "scripts/h.py",
        "scripts/i.py",
    ]
    .into_iter()
    .enumerate()
    {
        base.insert(
            path.to_owned(),
            (0o100_644, format!("script {index}\n").into_bytes()),
        );
    }
    base
}

fn fixture() -> AdoptionFixture {
    let base = base_inventory();
    let transition = transition(&base);
    let base_files = base
        .iter()
        .map(|(path, (mode, bytes))| HomebrewAdoptionTreeFile {
            path: path.clone(),
            mode: *mode,
            blob_sha: fake_blob_sha(bytes),
        })
        .collect::<Vec<_>>();
    let locator_bytes = locator().canonical_bytes().expect("locator bytes");
    let generated = BTreeMap::from([
        (
            ".github/workflows/ci.yml".to_owned(),
            b"workflow\n".to_vec(),
        ),
        (".velnor/config.toml".to_owned(), b"config\n".to_vec()),
        (
            ".velnor/release-manifest.json".to_owned(),
            br#"{"schema":1,"version":"0.1.55"}"#.to_vec(),
        ),
        ("mise.lock".to_owned(), b"lock\n".to_vec()),
        ("mise.toml".to_owned(), b"mise\n".to_vec()),
    ]);
    let mut candidate_bytes = generated.clone();
    candidate_bytes.insert("AGENTS.md".to_owned(), b"policy\n".to_vec());
    candidate_bytes.insert("CLAUDE.md".to_owned(), b"policy\n".to_vec());
    candidate_bytes.insert(
        crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH.to_owned(),
        locator_bytes.clone(),
    );
    for (path, (_, bytes)) in &base {
        if !transition.deletions.iter().any(|item| &item.path == path)
            && !matches!(path.as_str(), "CLAUDE.md" | "AGENTS.md")
            && !transition.generated_output_paths.contains(path)
        {
            candidate_bytes.insert(path.clone(), bytes.clone());
        }
    }
    let candidate_files = candidate_bytes
        .iter()
        .map(|(path, bytes)| HomebrewAdoptionTreeFile {
            path: path.clone(),
            mode: 0o100_644,
            blob_sha: fake_blob_sha(bytes),
        })
        .collect::<Vec<_>>();
    (
        transition,
        base_files,
        candidate_files,
        b"policy\n".to_vec(),
        generated,
        candidate_bytes,
    )
}

fn changes(
    transition: &HomebrewAdoptionTransition,
    generated: &BTreeMap<String, Vec<u8>>,
    claude: &[u8],
    locator: &[u8],
) -> Vec<HomebrewAdoptionTreeChange> {
    let mut changes = transition
        .deletions
        .iter()
        .map(|item| HomebrewAdoptionTreeChange {
            path: item.path.clone(),
            old_mode: Some(item.mode),
            new_mode: None,
            old_blob_sha: Some(item.blob_sha.clone()),
            new_blob_sha: None,
            status: "D".to_owned(),
        })
        .collect::<Vec<_>>();
    changes.push(HomebrewAdoptionTreeChange {
        path: "CLAUDE.md".to_owned(),
        old_mode: Some(0o120_000),
        new_mode: Some(0o100_644),
        old_blob_sha: Some(fake_blob_sha(b"AGENTS.md")),
        new_blob_sha: Some(fake_blob_sha(claude)),
        status: "T".to_owned(),
    });
    changes.push(HomebrewAdoptionTreeChange {
        path: crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH.to_owned(),
        old_mode: None,
        new_mode: Some(0o100_644),
        old_blob_sha: None,
        new_blob_sha: Some(fake_blob_sha(locator)),
        status: "A".to_owned(),
    });
    changes.extend(
        generated
            .iter()
            .map(|(path, bytes)| HomebrewAdoptionTreeChange {
                path: path.clone(),
                old_mode: Some(0o100_644),
                new_mode: Some(0o100_644),
                old_blob_sha: Some(fake_blob_sha(b"old generated")),
                new_blob_sha: Some(fake_blob_sha(bytes)),
                status: "M".to_owned(),
            }),
    );
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

#[test]
fn accepts_only_exact_seed_tree_and_renderer_bytes() {
    let (transition, _, candidate, claude, generated, bytes) = fixture();
    let locator = bytes
        .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
        .expect("locator");
    let evidence = transition
        .validate_candidate_tree(HomebrewAdoptionCandidateInput {
            base: (&transition.base_sha, &transition.base_tree),
            head: (&"c".repeat(40), &"d".repeat(40)),
            changes: &changes(&transition, &generated, &claude, locator),
            candidate_files: &candidate,
            claude: (&claude, &claude),
            locator,
            generated_files: &generated,
            candidate_file_bytes: &bytes,
        })
        .expect("valid seed");
    assert_eq!(evidence.transition_id, "initial-seed");
    assert_eq!(evidence.changed_paths.len(), 16);
}

#[test]
fn rejects_unlisted_path_and_changed_inert_formula() {
    let (transition, _, mut candidate, claude, generated, mut bytes) = fixture();
    candidate.push(HomebrewAdoptionTreeFile {
        path: "scripts/surprise.py".to_owned(),
        mode: 0o100_644,
        blob_sha: fake_blob_sha(b"surprise"),
    });
    bytes.insert("scripts/surprise.py".to_owned(), b"surprise".to_vec());
    assert!(
        transition
            .validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (&"c".repeat(40), &"d".repeat(40)),
                changes: &{
                    let mut changes = changes(
                        &transition,
                        &generated,
                        &claude,
                        bytes
                            .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                            .expect("locator"),
                    );
                    changes.push(HomebrewAdoptionTreeChange {
                        path: "scripts/surprise.py".to_owned(),
                        old_mode: None,
                        new_mode: Some(0o100_644),
                        old_blob_sha: None,
                        new_blob_sha: Some(fake_blob_sha(b"surprise")),
                        status: "A".to_owned(),
                    });
                    changes.sort_by(|left, right| left.path.cmp(&right.path));
                    changes
                },
                candidate_files: &candidate,
                claude: (&claude, &claude),
                locator: bytes
                    .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                    .expect("locator"),
                generated_files: &generated,
                candidate_file_bytes: &bytes,
            },)
            .is_err()
    );
}

#[test]
fn rejects_changed_inert_formula() {
    let (transition, _, mut candidate, claude, generated, mut bytes) = fixture();
    let path = "Formula/jackin.rb";
    let changed = b"changed formula\n".to_vec();
    let entry = candidate
        .iter_mut()
        .find(|item| item.path == path)
        .expect("formula");
    entry.blob_sha = fake_blob_sha(&changed);
    bytes.insert(path.to_owned(), changed);
    assert!(
        transition
            .validate_candidate_tree(HomebrewAdoptionCandidateInput {
                base: (&transition.base_sha, &transition.base_tree),
                head: (&"c".repeat(40), &"d".repeat(40)),
                changes: &changes(
                    &transition,
                    &generated,
                    &claude,
                    bytes
                        .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                        .expect("locator"),
                ),
                candidate_files: &candidate,
                claude: (&claude, &claude),
                locator: bytes
                    .get(crate::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
                    .expect("locator"),
                generated_files: &generated,
                candidate_file_bytes: &bytes,
            },)
            .is_err()
    );
}
