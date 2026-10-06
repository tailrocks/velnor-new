//! Literal native role inputs; credential grants are separate owner operations.
use super::super::triggers;
use std::collections::BTreeMap;
use velnor_actions_contract::config::AptDeliveryConfig;
use velnor_actions_native::apt::AptOperation;

pub(super) fn environment(
    operation: AptOperation,
    config: &AptDeliveryConfig,
) -> BTreeMap<String, String> {
    let mut result = match operation {
        AptOperation::Verify => pairs(&[
            ("CHANNEL", "${{ inputs.channel || 'stable' }}"),
            ("INPUT_VERSION", "${{ inputs.version || '' }}"),
            ("INPUT_COMMIT", "${{ inputs.commit || '' }}"),
            ("GH_TOKEN", "${{ github.token }}"),
        ]),
        AptOperation::Stage => pairs(&[
            ("INPUT_SUITE", "${{ inputs.channel || 'stable' }}"),
            ("DELIVERY_MODE", "${{ inputs.mode || 'publish' }}"),
            ("APT_GPG_PRIVATE_KEY", "${{ secrets.APT_GPG_PRIVATE_KEY }}"),
            ("APT_GPG_PASSPHRASE", "${{ secrets.APT_GPG_PASSPHRASE }}"),
        ]),
        AptOperation::IncomingTransport => pairs(&[
            ("ARTIFACT_ID", "${{ needs.verify.outputs.artifact_id }}"),
            (
                "ARTIFACT_DIGEST",
                "${{ needs.verify.outputs.artifact_digest }}",
            ),
            ("GH_TOKEN", "${{ github.token }}"),
        ]),
        AptOperation::PagesAdmission => pages(config),
        AptOperation::Result => {
            let mut environment = pairs(&[
                ("VERIFY", "${{ needs.verify.result }}"),
                ("ADMIT", "${{ needs.admit.result }}"),
                ("STAGE", "${{ needs.stage.result }}"),
                ("DEPLOY", "${{ needs.deploy.result }}"),
            ]);
            environment.insert(
                "PUBLICATION_ELIGIBLE".to_owned(),
                triggers::publication_condition(&config.consumer_repository, &config.branch),
            );
            environment
        }
    };
    if operation != AptOperation::Result {
        result.extend(pairs(&[
            ("GITHUB_REPOSITORY", "${{ github.repository }}"),
            ("GITHUB_SHA", "${{ github.sha }}"),
            ("GITHUB_RUN_ID", "${{ github.run_id }}"),
            ("GITHUB_RUN_ATTEMPT", "${{ github.run_attempt }}"),
            ("GITHUB_EVENT_NAME", "${{ github.event_name }}"),
            ("GITHUB_REF", "${{ github.ref }}"),
            ("GITHUB_SERVER_URL", "https://github.com"),
            ("GITHUB_API_URL", "https://api.github.com"),
        ]));
    }
    result
}

fn pages(config: &AptDeliveryConfig) -> BTreeMap<String, String> {
    let mut environment = pairs(&[
        ("INPUT_SUITE", "${{ inputs.channel || 'stable' }}"),
        ("DELIVERY_MODE", "${{ inputs.mode || 'publish' }}"),
        (
            "EXPECTED_ARTIFACT_ID",
            "${{ needs.stage.outputs.artifact_id }}",
        ),
        (
            "EXPECTED_ARTIFACT_DIGEST",
            "${{ needs.stage.outputs.artifact_digest }}",
        ),
        ("FULL_CI_RESULT", "${{ needs.admit.result }}"),
        ("APPROVED_SOURCE_SHA", "${{ github.sha }}"),
        ("ADMISSION_EVENT_POLICY", "default-branch"),
        ("GH_TOKEN", "${{ github.token }}"),
    ]);
    environment.insert(
        "APPROVED_REPOSITORY".to_owned(),
        config.consumer_repository.clone(),
    );
    environment.insert("APPROVED_DEFAULT_BRANCH".to_owned(), config.branch.clone());
    environment
}

fn pairs(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}
