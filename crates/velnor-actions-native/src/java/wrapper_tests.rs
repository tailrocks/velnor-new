use super::*;
const SCRIPT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const JAR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DISTRIBUTION: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn authority() -> WrapperAuthority<'static> {
    WrapperAuthority {
        engine_version: "9.5.1",
        script_sha256: SCRIPT,
        jar_sha256: JAR,
        distribution_sha256: DISTRIBUTION,
    }
}

fn canonical_properties() -> String {
    "distributionBase=GRADLE_USER_HOME\ndistributionPath=wrapper/dists\ndistributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\ndistributionSha256Sum=distribution-authority\nnetworkTimeout=10000\nvalidateDistributionUrl=true\nzipStoreBase=GRADLE_USER_HOME\nzipStorePath=wrapper/dists\n".replace("distribution-authority", DISTRIBUTION)
}

#[test]
fn bytes_mode_and_distribution_must_match_exact_authority() {
    let pins = authority();
    let properties = canonical_properties();
    let mut evidence = WrapperEvidence {
        script_sha256: SCRIPT,
        script_executable: true,
        daemon_criteria_present: false,
        jar_sha256: JAR,
        properties: &properties,
    };
    assert!(validate_wrapper(&pins, &evidence).is_ok());
    evidence.daemon_criteria_present = true;
    assert!(validate_wrapper(&pins, &evidence).is_err());
    evidence.daemon_criteria_present = false;
    evidence.script_executable = false;
    assert!(validate_wrapper(&pins, &evidence).is_err());
    evidence.script_executable = true;
    evidence.script_sha256 = "other-script";
    assert!(validate_wrapper(&pins, &evidence).is_err());
    evidence.script_sha256 = SCRIPT;
    evidence.jar_sha256 = "other-jar";
    assert!(validate_wrapper(&pins, &evidence).is_err());
}

#[test]
fn properties_refuse_altered_authority_and_noncanonical_syntax() {
    let pins = authority();
    let canonical = canonical_properties();
    assert!(validate_properties(&canonical, &pins).is_ok());
    for changed in [
        canonical.replace("9.5.1", "9.7.0"),
        canonical.replace("services.gradle.org", "example.invalid"),
        canonical.replace(DISTRIBUTION, "other-distribution"),
        canonical.replace(
            "validateDistributionUrl=true",
            "validateDistributionUrl=false",
        ),
        format!("{canonical}unexpected=value\n"),
        format!("{canonical}distributionUrl=other\n"),
        canonical.replace("distributionUrl=", "distributionUrl:"),
        canonical.replace("zipStorePath=wrapper/dists", "zipStorePath=wrapper/dists "),
        canonical.replace("networkTimeout=10000", "networkTimeout=10000\t"),
        canonical
            .lines()
            .filter(|line| !line.starts_with("distributionSha256Sum="))
            .map(|line| format!("{line}\n"))
            .collect(),
    ] {
        assert!(validate_properties(&changed, &pins).is_err());
    }
    assert!(properties("key=a\nkey=b").is_err());
}
