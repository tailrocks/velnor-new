use super::{PROPERTIES, bundle_mode_config, preview, repo, section};
use crate::impl_common::TestResult;

const REASON: &str = "private repository on a plan without artifact attestations";

fn disabled_config(config: &str) -> String {
    let maven_fields =
        format!("root = \"backend\"\nattest = false\nattest_reason = \"{REASON}\"\n");
    config.replace("root = \"backend\"\n", &maven_fields)
}

#[test]
fn maven_opt_out_precedes_both_provenance_modes() -> TestResult {
    for config in [
        disabled_config(super::CONFIG),
        disabled_config(&bundle_mode_config()),
    ] {
        let generated = preview(repo(&config, PROPERTIES)?.path())?;
        let attest = section(&generated, "maven-attest-processor-shared-libs")?;
        assert!(
            attest.contains("Provenance attestation disabled by config"),
            "{attest}"
        );
        assert!(attest.contains(REASON), "{attest}");
        assert!(!attest.contains("attest-build-provenance"), "{attest}");
        assert!(
            !attest.contains("cosign") && !attest.contains("attestation verify"),
            "{attest}"
        );

        let publish = section(&generated, "maven-publish-processor-shared-libs")?;
        assert!(publish.contains("Upload release assets"), "{publish}");
        assert!(!publish.contains("Download provenance bundle"), "{publish}");
        assert!(!publish.contains("Verify provenance bundle"), "{publish}");
        assert!(
            !publish.contains("sigstore.json") && !publish.contains("provenance.json"),
            "{publish}"
        );
    }
    Ok(())
}
