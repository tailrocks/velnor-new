use velnor_actions_dbt::{PackageSource, parse_packages, verify_lock};

const MANIFEST: &str =
    "# pins\npackages:\n  - package: dbt-labs/dbt_utils\n    version: [\">=1.1.0\", \"<2.0.0\"]\n";
const LOCK: &str = "packages:\n- package: dbt-labs/dbt_utils\n  name: dbt_utils\n  version: 1.4.1\nsha1_hash: cc41\n";

fn codes(manifest: &str, lock: Option<&str>) -> Vec<String> {
    let parsed = parse_packages(manifest).and_then(|manifest| {
        let lock = lock.map(parse_packages).transpose()?;
        Ok(verify_lock(&manifest, lock.as_deref()))
    });
    match parsed {
        Ok(findings) => findings.iter().map(|f| f.code().to_owned()).collect(),
        Err(error) => vec![format!("unparsed {}", error.code())],
    }
}

#[test]
fn the_consumer_manifest_and_lock_agree() {
    let manifest = parse_packages(MANIFEST).expect("manifest");
    assert_eq!(
        manifest[0].source,
        PackageSource::Hub("dbt-labs/dbt_utils".into())
    );
    assert_eq!(codes(MANIFEST, Some(LOCK)).len(), 0);
}

#[test]
fn a_missing_lock_or_entry_is_a_finding() {
    assert_eq!(codes(MANIFEST, None), ["dbt_package_lock_missing"]);
    assert_eq!(
        codes(
            MANIFEST,
            Some("packages:\n- package: other/pkg\n  version: 1.0.0\n")
        ),
        ["dbt_package_not_locked:dbt-labs/dbt_utils"]
    );
}

#[test]
fn git_packages_need_a_full_revision_and_hub_packages_an_exact_version() {
    let git = "packages:\n  - git: https://github.com/a/b.git\n    revision: main\n";
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let ok = format!("packages:\n- git: https://github.com/a/b.git\n  revision: {sha}\n");
    let bad = "packages:\n- git: https://github.com/a/b.git\n  revision: main\n";
    assert_eq!(codes(git, Some(&ok)).len(), 0);
    assert_eq!(
        codes(git, Some(bad)),
        ["dbt_package_lock_unpinned:https://github.com/a/b.git"]
    );
    assert_eq!(
        codes(MANIFEST, Some("packages:\n- package: dbt-labs/dbt_utils\n")),
        ["dbt_package_lock_unpinned:dbt-labs/dbt_utils"]
    );
}

#[test]
fn local_packages_need_no_lock_but_must_stay_inside() {
    assert_eq!(codes("packages:\n  - local: vendored/pkg\n", None).len(), 0);
    assert_eq!(
        codes("packages:\n  - local: ../shared\n", None),
        ["dbt_local_package_escapes_project:../shared"]
    );
}

#[test]
fn unsupported_sources_and_shapes_fail_closed() {
    let error = parse_packages("packages:\n  - tarball: https://x/y.tgz\n    name: y\n")
        .expect_err("tarball");
    assert_eq!(error.code(), "dbt_package_source_unsupported:tarball");
    let error = parse_packages("packages:\n  - package: a/b\n weird\n").expect_err("shape");
    assert!(error.code().starts_with("dbt_packages_unparsed"));
    assert_eq!(parse_packages("name: x\n").expect("no key").len(), 0);
}
