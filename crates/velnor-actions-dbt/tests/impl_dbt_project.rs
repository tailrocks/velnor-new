use velnor_actions_dbt::{
    ProjectFiles, analyze_project, check_profiles, job_id, job_name, slug, uses_postgres_adapter,
};

const PROJECT: &str = "name: 'chainargos_dbt'\nversion: '1.0.0'\nprofile: 'chainargos'\nmodel-paths: [\"models\"]\nmacro-paths: [\"macros\"]\ntarget-path: \"target\"\nclean-targets:\n  - \"target\"\n";

fn codes(findings: &[velnor_actions_dbt::DbtError]) -> Vec<&str> {
    findings
        .iter()
        .map(velnor_actions_dbt::DbtError::code)
        .collect()
}

#[test]
fn a_clean_project_has_identity_and_no_findings() {
    let files = ProjectFiles {
        project: PROJECT,
        ..ProjectFiles::default()
    };
    let (project, findings) = analyze_project("chainargos-dbt", &files);
    assert_eq!(findings.len(), 0);
    let project = project.expect("project");
    assert_eq!(
        (project.name.as_str(), project.profile.as_str()),
        ("chainargos_dbt", "chainargos")
    );
    assert_eq!(job_id(&project), "dbt-chainargos-dbt");
    assert_eq!(job_name(&project), "dbt / chainargos-dbt");
}

#[test]
fn bad_names_and_escaping_paths_are_findings() {
    let text = "name: 1bad\nprofile: ok\nmodel-paths: [\"../models\"]\ntarget-path: /abs\n";
    let (project, findings) = analyze_project(
        ".",
        &ProjectFiles {
            project: text,
            ..ProjectFiles::default()
        },
    );
    assert!(project.is_none());
    assert_eq!(
        codes(&findings),
        [
            "dbt_path_escapes_project:model-paths",
            "dbt_path_escapes_project:target-path",
            "dbt_project_name_invalid:1bad",
        ]
    );
}

#[test]
fn package_policy_runs_through_the_project() {
    let files = ProjectFiles {
        project: PROJECT,
        packages: Some("packages:\n  - package: a/b\n    version: 1.0.0\n"),
        ..ProjectFiles::default()
    };
    let (project, findings) = analyze_project("p", &files);
    assert!(project.is_some());
    assert_eq!(codes(&findings), ["dbt_package_lock_missing"]);
}

#[test]
fn literal_credentials_in_profiles_are_findings() {
    let ok = "p:\n  outputs:\n    dev:\n      user: \"{{ env_var('DBT_USER') }}\"\n      password: \"{{ env_var('DBT_PASSWORD') }}\"\n";
    assert_eq!(check_profiles(ok).len(), 0);
    let bad = "p:\n  outputs:\n    dev:\n      password: hunter2\n      token: abc # c\n";
    let found = check_profiles(bad);
    assert_eq!(
        codes(&found),
        [
            "dbt_profile_literal_secret:password",
            "dbt_profile_literal_secret:token"
        ]
    );
}

#[test]
fn slug_folds_directories() {
    assert_eq!(slug("."), "root");
    assert_eq!(slug("Analytics/Core_1"), "analytics-core-1");
}

#[test]
fn only_a_postgres_type_output_counts_as_the_postgres_adapter() {
    let pg = "w:\n  outputs:\n    ci:\n      type: postgres\n      host: \"{{ env_var('DBT_HOST') }}\"\n";
    assert!(uses_postgres_adapter(pg));
    assert!(uses_postgres_adapter(
        "w:\n  outputs:\n    ci:\n      type: \"postgres\" # x\n"
    ));
    assert!(!uses_postgres_adapter(
        "w:\n  outputs:\n    ci:\n      type: snowflake\n"
    ));
    assert!(!uses_postgres_adapter(
        "w:\n  outputs:\n    ci:\n      host: postgres\n"
    ));
    assert!(!uses_postgres_adapter("# type: postgres\n"));
}
