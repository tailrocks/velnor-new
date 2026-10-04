//! Dedicated Xcode/tap paths require exact typed source coverage.

use super::*;
use velnor_actions_contract::build_index_from_list;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn config_with_files(
    files: &[&str],
) -> Result<(tempfile::TempDir, VelnorConfig, FileIndex), Box<dyn std::error::Error>> {
    let repo = tempfile::TempDir::new()?;
    std::fs::create_dir_all(repo.path().join(".velnor"))?;
    std::fs::write(repo.path().join(".velnor/config.toml"), "schema = 1\n")?;
    for relative in files {
        let path = repo.path().join(relative);
        std::fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        std::fs::write(path, "source fixture\n")?;
    }
    let config = crate::config::load_config(repo.path())?;
    let paths = files
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let index = build_index_from_list(repo.path(), &paths, &[])?;
    Ok((repo, config, index))
}

fn native_xcode() -> Result<WorkloadConfig, serde_json::Error> {
    let mut workload = super::tests::profile_workload("products/second", "apple/client")?;
    workload.kind = WorkloadKind::NativeXcodeProjectCi;
    workload
        .native_desktop
        .as_mut()
        .expect("test profile")
        .apple = Some(serde_json::from_value(serde_json::json!({
        "project_spec": "project.yml", "project_path": "Second.xcodeproj",
        "scheme": "SecondApp", "app_name": "SecondApp",
        "bundle_identifier": "org.example.second", "bundle_name": "Second App",
        "app_path": "build/Second.app", "derived_data_path": "build/derived",
        "archive_name_prefix": "second"
    }))?);
    Ok(workload)
}

#[test]
fn deleted_xcode_declaration_cannot_become_empty_success() -> TestResult {
    let exact = "products/second/apple/client/Second.xcodeproj/project.pbxproj";
    let (_repo, mut config, index) = config_with_files(&[exact])?;
    assert!(qualify(&config, &index).is_err());
    config.stacks.workloads.push(native_xcode()?);
    qualify(&config, &index)?;
    config.stacks.workloads[0]
        .native_desktop
        .as_mut()
        .ok_or("no profile")?
        .apple
        .as_mut()
        .ok_or("no Apple profile")?
        .project_path = "Other.xcodeproj".to_owned();
    assert!(qualify(&config, &index).is_err());
    Ok(())
}

#[test]
fn formula_and_cask_sources_need_exact_coverage_without_forcing_audit() -> TestResult {
    let (_repo, mut config, index) = config_with_files(&["Formula/tool.rb", "Casks/app.rb"])?;
    assert!(qualify(&config, &index).is_err());
    let syntax: WorkloadConfig = serde_json::from_value(serde_json::json!({
        "name": "tap-syntax", "kind": "ruby_syntax", "paths": ["Casks/app.rb", "Formula/tool.rb"]
    }))?;
    config.stacks.workloads.push(syntax);
    qualify(&config, &index)?;
    config.stacks.workloads[0].paths.pop();
    assert!(qualify(&config, &index).is_err());
    config.stacks.workloads.clear();
    config
        .stacks
        .workloads
        .push(serde_json::from_value(serde_json::json!({
            "name": "tap-audit", "kind": "homebrew_audit"
        }))?);
    qualify(&config, &index)?;
    Ok(())
}

#[test]
fn generic_ruby_yaml_and_dependency_brewfile_do_not_invent_obligations() -> TestResult {
    let (_repo, config, index) = config_with_files(&[
        "tools/example.rb",
        "project.yml",
        "Brewfile",
        "nested/Formula/example.rb",
        "sample.project/project.pbxproj",
        "Formula/README.md",
    ])?;
    qualify(&config, &index)?;
    Ok(())
}
