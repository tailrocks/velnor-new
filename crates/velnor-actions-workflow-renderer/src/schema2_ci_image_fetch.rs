// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ProductReleasePins,
    ci_image_producer::{
        SDK_CI_IMAGE_ARTIFACT_SCRIPT, SDK_CI_IMAGE_DOWNLOAD_SCRIPT, SDK_CI_IMAGE_LOOKUP_SCRIPT,
    },
};
use crate::{RenderError, commands::join_argv_for_run, yaml::Yaml};

#[derive(Clone, Copy)]
pub(super) enum CandidateFamily {
    Job,
    Runner,
}

pub(super) fn step(
    pins: &ProductReleasePins,
    family: CandidateFamily,
) -> Result<Yaml, RenderError> {
    let gh = join_argv_for_run(&pins.gh_argv)?;
    let (artifact, destination, files) = match family {
        CandidateFamily::Job => (
            "velnor-job-image-candidate-$ci_attempt",
            "$RUNNER_TEMP/privatecandidate",
            "job-image.oci.tar job-image-candidate.json job-image-native-source-closure.json",
        ),
        CandidateFamily::Runner => (
            "image-assets-$ci_attempt",
            "$RUNNER_TEMP/runner-candidates",
            "runner.oci.tar dind.oci.tar resource-probe.oci.tar runner-image-publication.json dind-image-publication.json resource-probe-image-publication.json",
        ),
    };
    let download = SDK_CI_IMAGE_DOWNLOAD_SCRIPT.replace(
        "__VELNOR_ARTIFACT_EXTRACTOR__",
        SDK_CI_IMAGE_ARTIFACT_SCRIPT,
    );
    let mut run = format!(
        "set -euo pipefail\ngh() {{ {gh} \"$@\"; }}\nrepo=tailrocks/velnor-new\nsha=\"$GITHUB_SHA\"\n[[ \"$GITHUB_REPOSITORY\" == \"$repo\" && \"$GITHUB_EVENT_NAME\" == push && \"$GITHUB_REF\" == refs/heads/main && \"${{GITHUB_REF_PROTECTED:-}}\" == true ]]\n[[ \"$sha\" =~ ^[0-9a-f]{{40}}$ && \"$GITHUB_WORKFLOW_SHA\" == \"$sha\" ]]\nbase=\"$RUNNER_TEMP/velnor-ci-candidate-fetch\"\n[[ ! -e \"$base\" ]]\numask 077\nmkdir -m 700 \"$base\"\n"
    );
    run.push_str(SDK_CI_IMAGE_LOOKUP_SCRIPT);
    run.push_str(&download);
    run.push_str("ready=false\nretry=0\nwhile [ \"$retry\" -lt 300 ]; do\n  if ci=$(list_runs ci.yml); then ready=true; break; fi\n  retry=$((retry + 1))\n  sleep 10\ndone\n[[ \"$ready\" == true ]]\nci_id=$(printf '%s\\n' \"$ci\" | cut -f1)\nci_attempt=$(printf '%s\\n' \"$ci\" | cut -f2)\nvalidate_required \"$ci_id\" \"$ci_attempt\"\n");
    run = format!(
        "{run}artifact=\"{artifact}\"\nartifact_id=$(find_artifact \"$ci_id\" \"$artifact\")\ndestination=\"{destination}\"\n[[ ! -e \"$destination\" ]]\nmkdir -m 700 \"$destination\"\ndownload_artifact \"$artifact_id\" \"$destination\" {files}\n"
    );
    run.push_str("[[ \"$(list_runs ci.yml)\" == \"$ci\" ]]\nprintf 'producer_run_id=%s\\nproducer_run_attempt=%s\\nartifact_id=%s\\n' \"$ci_id\" \"$ci_attempt\" \"$artifact_id\" >> \"$GITHUB_OUTPUT\"\n");
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Fetch exact successful CI candidates"),
        ),
        ("id".to_owned(), Yaml::str("locate")),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ]))
}
