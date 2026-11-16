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

#[test]
fn release_record_accepts_a_tag_with_build_metadata() {
    let plus = release("app", "app-v1.2.3+linux");
    assert_eq!(
        recorded(&[("app.json", plus.as_str())]),
        Ok("app=app-v1.2.3+linux\n".to_owned())
    );
}

#[test]
fn every_tag_the_plan_renders_is_a_tag_the_recorder_accepts() -> Result<(), String> {
    let files = velnor_release::ReleaseFiles {
        release_toml: None,
        release_plz_toml: [(
            ".".to_owned(),
            "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n[[package]]\nname = \"app\"\n".to_owned(),
        )]
        .into(),
        mise_tomls: vec!["[tools]\nrelease-plz = \"0.4.3\"\n".to_owned()],
        mise_lock: "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[tools.release-plz.\"platforms.linux-x64\"]\nchecksum = \"sha256:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90\"\nurl = \"https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz\"\n".to_owned(),
        ..velnor_release::ReleaseFiles::default()
    };
    let plan = velnor_release::ReleasePlan::read(&files).map_err(|e| e.to_string())?;
    let pattern = plan.roots[0].packages[0].1.as_str().to_owned();
    for version in ["1.2.3", "1.4.0-rc.1", "1.2.3+linux", "1.2.3-rc.1+build.5"] {
        let tag = pattern.replace("{{ version }}", version);
        assert!(velnor_ci::release::PlainName::tag(&tag).is_ok(), "{tag}");
    }
    Ok(())
}
