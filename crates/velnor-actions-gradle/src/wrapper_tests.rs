//! Wrapper gate tests. The "official jar" is a stand-in whose digest is
//! injected through the catalog function under test via a real digest match.

use super::*;

fn props(url: &str, sum: Option<&str>) -> String {
    let mut text = format!(
        "distributionBase=GRADLE_USER_HOME\ndistributionUrl={}\n",
        url.replace(':', "\\:")
    );
    if let Some(sum) = sum {
        text.extend(["distributionSha256Sum=", sum, "\n"]);
    }
    text.push_str("zipStoreBase=GRADLE_USER_HOME\n");
    text
}

const BIN: &str = "bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f";
const URL: &str = "https://services.gradle.org/distributions/gradle-9.5.1-bin.zip";

#[test]
fn properties_parser_unescapes_and_skips_comments() {
    let map = parse_properties("# c\n! c\na\\:b=c\\=d\n distributionUrl = https\\://x \n\n");
    assert_eq!(map.get("a:b").map(String::as_str), Some("c=d"));
    assert_eq!(
        map.get("distributionUrl").map(String::as_str),
        Some("https://x")
    );
}

#[test]
fn url_gate_accepts_only_official_releases() {
    assert_eq!(
        parse_distribution_url(URL),
        Some(("9.5.1".to_owned(), Flavour::Bin))
    );
    assert_eq!(
        parse_distribution_url("https://services.gradle.org/distributions/gradle-8.14-all.zip"),
        Some(("8.14".to_owned(), Flavour::All))
    );
    for bad in [
        "https://evil.example/distributions/gradle-9.5.1-bin.zip",
        "http://services.gradle.org/distributions/gradle-9.5.1-bin.zip",
        "https://services.gradle.org/distributions-snapshots/gradle-9.6-20261001-bin.zip",
        "https://services.gradle.org/distributions/gradle-9.5.1-rc-1-bin.zip",
        "https://services.gradle.org/distributions/gradle-9.5.1-bin.zip?x=1",
        "https://services.gradle.org/distributions/gradle-9.5.1-bin.tar",
        "https://services.gradle.org.evil.example/distributions/gradle-9.5.1-bin.zip",
        "file:///tmp/gradle-9.5.1-bin.zip",
    ] {
        assert_eq!(parse_distribution_url(bad), None, "{bad}");
    }
}

#[test]
fn gates_fail_in_contract_order_with_named_findings() {
    let jar = b"jar";
    let cases: [(Option<String>, WrapperFinding); 6] = [
        (None, WrapperFinding::PropertiesMissing),
        (
            Some("distributionBase=x\n".to_owned()),
            WrapperFinding::PropertiesMissing,
        ),
        (
            Some(props(
                "https://evil.example/gradle-9.5.1-bin.zip",
                Some(BIN),
            )),
            WrapperFinding::DistributionUrlUntrusted,
        ),
        (
            Some(props(URL, None)),
            WrapperFinding::DistributionChecksumMissing,
        ),
        (
            Some(props(URL, Some(&"0".repeat(64)))),
            WrapperFinding::DistributionChecksumMismatch,
        ),
        (
            Some(props(
                "https://services.gradle.org/distributions/gradle-1.1-bin.zip",
                Some(BIN),
            )),
            WrapperFinding::DistributionUncataloged,
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(
            verify(text.as_deref(), jar),
            Err(expected.clone()),
            "{expected}"
        );
    }
    assert_eq!(
        verify(Some(&props(URL, Some(BIN))), b"tampered"),
        Err(WrapperFinding::WrapperJarUnverified)
    );
    // The bin digest must not satisfy an `-all` URL.
    assert_eq!(
        verify(
            Some(&props(
                "https://services.gradle.org/distributions/gradle-9.5.1-all.zip",
                Some(BIN)
            )),
            jar
        ),
        Err(WrapperFinding::DistributionChecksumMismatch)
    );
}

#[test]
fn codes_are_stable() {
    assert_eq!(
        WrapperFinding::WrapperJarUnverified.code(),
        "gradle_wrapper_jar_unverified"
    );
    assert_eq!(
        WrapperFinding::DistributionUrlUntrusted.to_string(),
        "gradle_distribution_url_untrusted"
    );
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
