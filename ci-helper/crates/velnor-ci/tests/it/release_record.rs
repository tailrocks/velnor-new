// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release::record;

pub(crate) const CI: &str = r#"{"releases":[{"package_name":"velnor-ci","prs":[{"html_url":"https://github.com/tailrocks/velnor-new/pull/760","number":760}],"tag":"velnor-ci-v0.1.80","version":"0.1.80"}]}"#;
pub(crate) const NOTHING: &str = r#"{"releases":[]}"#;
const TWO: &str = r#"{"releases":[{"package_name":"app_cli","prs":[],"tag":"app_cli-v0.2.0","version":"0.2.0"},{"package_name":"app","prs":[],"tag":"app-v1.4.0-rc.1","version":"1.4.0-rc.1"}]}"#;

fn release(package: &str, tag: &str) -> String {
    format!(
        r#"{{"releases":[{{"package_name":{},"prs":[],"tag":{},"version":"0.1.80"}}]}}"#,
        serde_json::Value::from(package),
        serde_json::Value::from(tag)
    )
}

fn recorded(files: &[(&str, &str)]) -> Result<String, String> {
    let files: Vec<(String, String)> = files
        .iter()
        .map(|(file, text)| ((*file).to_owned(), (*text).to_owned()))
        .collect();
    record(&files).map_err(|e| e.to_string())
}

#[test]
fn release_record_writes_one_output_per_package_that_release_plz_released_in_any_root() {
    assert_eq!(
        recorded(&[("ci-helper.json", CI), ("generator.json", NOTHING)]),
        Ok("velnor-ci=velnor-ci-v0.1.80\n".to_owned())
    );
    assert_eq!(
        recorded(&[("app.json", TWO), ("ci-helper.json", CI)]),
        Ok("app=app-v1.4.0-rc.1\napp_cli=app_cli-v0.2.0\nvelnor-ci=velnor-ci-v0.1.80\n".to_owned())
    );
    assert_eq!(recorded(&[("generator.json", NOTHING)]), Ok(String::new()));
}

#[test]
fn release_record_refuses_a_package_or_a_tag_released_twice() {
    let later = release("velnor-ci", "velnor-ci-v0.1.81");
    let same = release("velnor-gen", "velnor-ci-v0.1.80");
    for (files, said) in [
        (
            [("ci-helper.json", CI), ("generator.json", later.as_str())],
            "release-plz released package velnor-ci twice, as velnor-ci-v0.1.80 and as velnor-ci-v0.1.81",
        ),
        (
            [("ci-helper.json", CI), ("again.json", CI)],
            "release-plz released package velnor-ci twice, as velnor-ci-v0.1.80 and as velnor-ci-v0.1.80",
        ),
        (
            [("ci-helper.json", CI), ("generator.json", same.as_str())],
            "release-plz released velnor-ci and velnor-gen with the same tag velnor-ci-v0.1.80",
        ),
    ] {
        assert_eq!(recorded(&files), Err(said.to_owned()));
    }
}

#[test]
fn release_record_accepts_semver_build_metadata_in_a_tag_but_not_in_a_package() {
    let text = release("app", "app-v1.2.3+build.5");
    assert_eq!(
        recorded(&[("app.json", &text)]),
        Ok("app=app-v1.2.3+build.5\n".to_owned())
    );
    let text = release("app+x", "app-v1.2.3");
    assert!(recorded(&[("app.json", &text)]).is_err());
}

#[test]
fn release_record_refuses_a_name_that_could_break_out_of_its_output_line() {
    for (package, tag) in [
        (
            "velnor-ci",
            "velnor-ci-v0.1.80\nvelnor-gen=velnor-gen-v6.6.6",
        ),
        ("velnor-ci", "velnor-ci-v$(id)"),
        ("velnor-ci", "-v0.1.80"),
        ("velnor-ci", "velnor-ci-v0.1.80 "),
        ("velnor-ci", ""),
        ("velnor-ci=x", "velnor-ci-v0.1.80"),
        ("velnor.ci", "velnor-ci-v0.1.80"),
        ("1velnor", "velnor-ci-v0.1.80"),
    ] {
        let text = release(package, tag);
        let said = recorded(&[("ci-helper.json", &text)]);
        assert!(
            said.as_ref()
                .is_err_and(|e| e.contains("must be ASCII letters") || e.contains("job output")),
            "{package:?} {tag:?}: {said:?}"
        );
    }
}

#[test]
fn release_record_refuses_a_file_that_is_not_release_plz_output() {
    for text in [
        "",
        "{}",
        "nothing to release",
        r#"{"releases":[{"package_name":"velnor-ci","version":"0.1.80"}]}"#,
    ] {
        let said = recorded(&[("ci-helper.json", CI), ("generator.json", text)]);
        assert!(
            said.as_ref().is_err_and(|e| e
                .starts_with("generator.json is not the output of release-plz release -o json")),
            "{text:?}: {said:?}"
        );
    }
}
