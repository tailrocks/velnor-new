//! Native preview TOML configuration decoding at the public loader boundary.
use super::PartialConfig;

#[test]
fn preview_section_survives_loader_materialization_independently_of_stable() {
    let partial: PartialConfig = toml::from_str(
        r#"
schema = 1
[stacks.rust.binary_release]
version_env = "JACKIN_VERSION_OVERRIDE"
[stacks.rust.binary_release.stable]
enabled = false
[stacks.rust.binary_release.preview]
enabled = true
[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"
"#,
    )
    .expect("native configuration");
    let config = partial.materialize().expect("materialized configuration");
    let release = &config.stacks.rust.expect("rust").binary_release;
    assert!(!release.stable.enabled);
    assert!(release.preview.enabled);
    assert_eq!(release.version_env, "JACKIN_VERSION_OVERRIDE");
    assert!(config.distribution.homebrew_tap.is_none());
}

#[test]
fn loader_rejects_evidence_disabling_and_unrestricted_distribution() {
    for source in [
        "schema = 1\n[stacks.rust.binary_release.evidence]\nprofile = 'signed-sbom-v1'\nchecksums = false\n",
        "schema = 1\n[distribution]\ncommand = 'publish'\n",
        "schema = 1\n[stacks.rust.binary_release.preview]\nversioning = 'shell'\n",
    ] {
        assert!(toml::from_str::<PartialConfig>(source).is_err(), "{source}");
    }
}
