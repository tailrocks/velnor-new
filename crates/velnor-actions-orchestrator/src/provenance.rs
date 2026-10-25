use crate::prepare::GenerationPreparation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProvenance {
    pub path: String,
    pub line: u32,
    pub command_or_setting: String,
    pub strength: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProvenance {
    pub workspace_root: String,
    pub compile_driver: String,
    pub driver_source: String,
    pub test_runner: String,
    pub runner_source: String,
    pub nextest_profile: String,
    pub nextest_config: Option<String>,
    pub evidence: Vec<EvidenceProvenance>,
}

pub(crate) fn profile_provenance(prep: &GenerationPreparation) -> Vec<ProfileProvenance> {
    prep.discovery
        .workspaces
        .iter()
        .map(|workspace| {
            let profile = &workspace.profile;
            ProfileProvenance {
                workspace_root: workspace.record.workspace_root.clone(),
                compile_driver: profile.compile_driver.as_str().to_owned(),
                driver_source: profile.driver_source.as_str().to_owned(),
                test_runner: profile.test_runner.as_str().to_owned(),
                runner_source: profile.runner_source.as_str().to_owned(),
                nextest_profile: profile.nextest_profile.as_str().to_owned(),
                nextest_config: profile.nextest_config.clone(),
                evidence: profile
                    .evidence
                    .iter()
                    .map(|sighting| EvidenceProvenance {
                        path: sighting.path.clone(),
                        line: sighting.line,
                        command_or_setting: sighting.command_or_setting.clone(),
                        strength: sighting.strength.as_str().to_owned(),
                    })
                    .collect(),
            }
        })
        .collect()
}
