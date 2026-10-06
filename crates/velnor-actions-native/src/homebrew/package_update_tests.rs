//! Regression proof using independent repository/output/artifact mappings.

use super::*;
use velnor_actions_contract::config::{
    PackageUpdateArchive, PackageUpdateArtifact, PackageUpdateOutput,
};

const UPDATER: &str = include_str!("../../tests/fixtures/package_update_source.sh");
const AUDITED_UPDATER: &str = include_str!("../../tests/fixtures/package_update_jackin_source.sh");

fn profile() -> PackageUpdateFixture {
    let targets = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
    ];
    let mut artifacts: Vec<_> = targets
        .into_iter()
        .map(|target| PackageUpdateArtifact {
            prefix: "nebula".to_owned(),
            target: target.to_owned(),
            archive: PackageUpdateArchive::TarGz,
            output: PackageUpdateOutput::Formula,
            preview: true,
            executable: target == "x86_64-unknown-linux-gnu",
        })
        .collect();
    artifacts.extend(targets[2..].iter().map(|target| PackageUpdateArtifact {
        prefix: "nebula-resource".to_owned(),
        target: (*target).to_owned(),
        archive: PackageUpdateArchive::TarGz,
        output: PackageUpdateOutput::Formula,
        preview: true,
        executable: false,
    }));
    artifacts.push(PackageUpdateArtifact {
        prefix: "nebula-ui".to_owned(),
        target: targets[0].to_owned(),
        archive: PackageUpdateArchive::Zip,
        output: PackageUpdateOutput::Cask,
        preview: false,
        executable: false,
    });
    PackageUpdateFixture {
        updater: "scripts/package-update.sh".to_owned(),
        repository: "orbit-tools/nebula".to_owned(),
        formula: "Formula/nebula.rb".to_owned(),
        preview_formula: "Formula/nebula-preview.rb".to_owned(),
        cask: Some("Casks/nebula-ui.rb".to_owned()),
        binary: "nebula".to_owned(),
        artifacts,
        supporting_manifests: vec!["nebula-resource-manifest.json".to_owned()],
    }
}

#[test]
fn profile_requires_all_source_inputs_and_repository_root() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::TempDir::new()?;
    let profile = profile();
    let paths = vec![
        profile.updater.clone(),
        profile.formula.clone(),
        profile.preview_formula.clone(),
        profile.cask.clone().ok_or("fixture cask")?,
        "scripts/other-source.sh".to_owned(),
    ];
    let index = velnor_actions_contract::build_index_from_list(root.path(), &paths, &[])?;
    validate_evidence(&profile, ".", &index)?;
    assert_eq!(inputs(&index), index.files());
    assert!(validate_evidence(&profile, "scripts", &index).is_err());
    for missing in &paths[..4] {
        let remaining: Vec<_> = paths
            .iter()
            .filter(|path| *path != missing)
            .cloned()
            .collect();
        let incomplete =
            velnor_actions_contract::build_index_from_list(root.path(), &remaining, &[])?;
        let error = validate_evidence(&profile, ".", &incomplete).expect_err("missing source");
        assert!(error.to_string().contains(missing), "{error}");
    }
    Ok(())
}

#[test]
fn helper_sources_and_arguments_have_one_compiled_owner() -> Result<(), Box<dyn std::error::Error>>
{
    let profile = profile();
    let args = arguments(&profile)?;
    assert_eq!(args.len(), 1);
    assert_eq!(
        serde_json::from_str::<PackageUpdateFixture>(&args[0])?,
        profile
    );
    let sources = support("0.1.0")?;
    assert_eq!(sources.files().len(), 1);
    assert_eq!(sources.files()[0].path(), OPERATION.path());
    assert!(sources.files()[0].source().contains(&script::compiled()));
    for forbidden in ["jackin", "test-package-update.sh", "mise run", "eval "] {
        assert!(
            !sources.files()[0].source().contains(forbidden),
            "{forbidden}"
        );
    }
    assert_eq!(tools(), [PackageUpdateTool::Ruby, PackageUpdateTool::Jq]);
    assert_eq!(rank(PHASE), Some(3));
    assert!(step_name(PHASE).is_some());
    Ok(())
}

#[test]
fn data_cannot_escape_paths_or_change_the_case_graph() {
    let mut profile = profile();
    assert!(profile.validate("fixture", "package_update").is_ok());
    profile.updater = "scripts/../other.sh".to_owned();
    assert!(profile.validate("fixture", "package_update").is_err());
    profile = self::profile();
    profile.artifacts[0].prefix = "$(command)".to_owned();
    assert!(profile.validate("fixture", "package_update").is_err());
    profile = self::profile();
    profile.artifacts.push(profile.artifacts[0].clone());
    assert!(profile.validate("fixture", "package_update").is_err());
    profile = self::profile();
    profile.artifacts[3].executable = false;
    assert!(profile.validate("fixture", "package_update").is_err());
    profile = self::profile();
    profile
        .supporting_manifests
        .push("identity.json".to_owned());
    assert!(profile.validate("fixture", "package_update").is_err());
}

fn execute(source: &str) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    execute_with_profile(source, &profile())
}

fn execute_with_profile(
    source: &str,
    profile: &PackageUpdateFixture,
) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let workspace = tempfile::TempDir::new()?;
    let root = workspace.path().join("source");
    let runner = workspace.path().join("runner");
    for directory in [
        root.join("scripts"),
        root.join("Formula"),
        root.join("Casks"),
        runner.clone(),
    ] {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(root.join(&profile.updater), source)?;
    for path in [&profile.formula, &profile.preview_formula]
        .into_iter()
        .chain(profile.cask.iter())
    {
        std::fs::write(root.join(path), "# source fixture\n")?;
    }
    let args = arguments(profile)?;
    Ok(std::process::Command::new("bash")
        .args(["-c", &script::compiled(), "velnor-package-fixtures"])
        .args(args)
        .current_dir(root)
        .env("RUNNER_TEMP", runner)
        .output()?)
}

#[test]
fn audited_source_runs_the_same_generator_owned_cases() -> Result<(), Box<dyn std::error::Error>> {
    let mut profile = profile();
    profile.repository = "jackin-project/jackin".to_owned();
    profile.formula = "Formula/jackin.rb".to_owned();
    profile.preview_formula = "Formula/jackin-preview.rb".to_owned();
    profile.cask = Some("Casks/jackin-desktop.rb".to_owned());
    profile.binary = "jackin".to_owned();
    for artifact in &mut profile.artifacts {
        artifact.prefix = match artifact.prefix.as_str() {
            "nebula-resource" => "jackin-capsule",
            "nebula-ui" => "jackin-desktop",
            _ => "jackin",
        }
        .to_owned();
    }
    profile.supporting_manifests = vec!["capsule-manifest.json".to_owned()];
    let result = execute_with_profile(AUDITED_UPDATER, &profile)?;
    let stdout = String::from_utf8(result.stdout)?;
    let stderr = String::from_utf8(result.stderr)?;
    assert!(result.status.success(), "{stdout}\n{stderr}");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("velnor-package-fixture-case:"))
            .count(),
        16
    );
    assert!(stdout.contains("velnor-package-fixture-complete: 16 cases"));
    Ok(())
}

#[test]
fn second_repository_runs_all_real_output_and_rejection_cases()
-> Result<(), Box<dyn std::error::Error>> {
    let result = execute(UPDATER)?;
    let stdout = String::from_utf8(result.stdout)?;
    let stderr = String::from_utf8(result.stderr)?;
    assert!(result.status.success(), "{stdout}\n{stderr}");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("velnor-package-fixture-case:"))
            .count(),
        16
    );
    assert!(stdout.contains("velnor-package-fixture-complete: 16 cases"));
    Ok(())
}

#[test]
fn successful_but_wrong_output_fails_generator_assertions() -> Result<(), Box<dyn std::error::Error>>
{
    let changed = UPDATER.replace("version \"$version\"", "version \"0.0.0\"");
    assert_ne!(changed, UPDATER);
    let result = execute(&changed)?;
    assert!(!result.status.success());
    assert!(!String::from_utf8(result.stdout)?.contains("velnor-package-fixture-complete"));
    Ok(())
}

#[test]
fn accepting_invalid_supporting_assets_fails_generator_case()
-> Result<(), Box<dyn std::error::Error>> {
    let changed = UPDATER.replace("manifest=\"$verified/release-manifest.json\"",
        "if [[ \"$verified\" == */missing-supporting ]]; then exit 0; fi\nmanifest=\"$verified/release-manifest.json\"");
    assert_ne!(changed, UPDATER);
    let result = execute(&changed)?;
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)?.contains("accepted invalid fixture: missing-supporting")
    );
    Ok(())
}

#[test]
fn swapped_artifact_checksums_fail_the_output_mapping() -> Result<(), Box<dyn std::error::Error>> {
    let changed = UPDATER
        .replace("sha256 \"$mac_arm\"", "sha256 \"$swap_placeholder\"")
        .replace("sha256 \"$mac_intel\"", "sha256 \"$mac_arm\"")
        .replace("sha256 \"$swap_placeholder\"", "sha256 \"$mac_intel\"");
    assert_ne!(changed, UPDATER);
    let result = execute(&changed)?;
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)?.contains("artifact URL/checksum mismatch"));
    Ok(())
}
