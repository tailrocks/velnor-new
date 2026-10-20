use velnor_actions_dbt::{FusionPlatform, check_mise_lock, fusion_url, parse_fusion_version};

fn lock(extra: &str, url_tail: &str) -> String {
    format!(
        "[[tools.dbt-fusion]]\nversion = \"2.0.0-preview.219\"\nbackend = \"aqua:getdbt.com/dbt-fusion\"\n\n[tools.dbt-fusion.\"platforms.linux-arm64\"]\nurl = \"https://public.cdn.getdbt.com/fs/cli/fs-v2.0.0-preview.219-aarch64-unknown-linux-gnu.tar.gz\"\n{extra}\n[tools.dbt-fusion.\"platforms.linux-x64\"]\nurl = \"https://public.cdn.getdbt.com/fs/cli/fs-v2.0.0-preview.219-x86_64-unknown-linux-gnu{url_tail}\"\n{extra}\n[tools.dbt-fusion.\"platforms.macos-arm64\"]\nurl = \"https://public.cdn.getdbt.com/fs/cli/fs-v2.0.0-preview.219-aarch64-apple-darwin.tar.gz\"\n{extra}\n"
    )
}

fn codes(text: &str) -> Vec<String> {
    match parse_fusion_version("2.0.0-preview.219") {
        Ok(version) => check_mise_lock(text, version)
            .iter()
            .map(|f| f.code().to_owned())
            .collect(),
        Err(error) => vec![error.to_string()],
    }
}

#[test]
fn urls_follow_the_single_admitted_template() {
    let version = parse_fusion_version("2.0.0-preview.219").expect("version");
    assert_eq!(
        fusion_url(version, FusionPlatform::LinuxX64),
        "https://public.cdn.getdbt.com/fs/cli/fs-v2.0.0-preview.219-x86_64-unknown-linux-gnu.tar.gz"
    );
}

#[test]
fn a_checksummed_lock_with_admitted_urls_is_clean() {
    let sum =
        "checksum = \"sha256:665039bfac4f458eebcfd7cc421df454020e2a39d7e3f3ba98addd7c0252d70e\"";
    assert_eq!(codes(&lock(sum, ".tar.gz")).len(), 0);
}

#[test]
fn the_consumer_lock_has_no_checksums() {
    assert_eq!(
        codes(&lock("", ".tar.gz")),
        [
            "dbt_fusion_checksum_missing:linux-arm64",
            "dbt_fusion_checksum_missing:linux-x64",
            "dbt_fusion_checksum_missing:macos-arm64",
        ]
    );
}

#[test]
fn foreign_urls_missing_rows_and_garbage_fail() {
    let sum = "checksum = \"sha256:aa\"";
    assert!(codes(&lock(sum, ".zip")).contains(&"dbt_fusion_url_untrusted:linux-x64".to_owned()));
    assert_eq!(codes("[tools]\n"), ["dbt_fusion_lock_missing"]);
    assert_eq!(codes("= nope"), ["dbt_fusion_lock_unparsed"]);
    let partial = "[[tools.dbt-fusion]]\nversion = \"2.0.0-preview.219\"\n";
    assert_eq!(codes(partial).len(), 3);
}
