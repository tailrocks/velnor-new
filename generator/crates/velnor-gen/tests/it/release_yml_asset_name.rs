// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, app, render, steps};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

fn named(template: &str, platforms: &str) -> Result<String, Box<dyn Error>> {
    let toml = format!(
        "[[binaries]]\npackage = \"app\"\nplatforms = [{platforms}]\nasset_name = {template:?}\n"
    );
    Ok(render(&[APP], Some(&toml))?.0)
}

#[test]
fn a_binary_without_an_asset_name_keeps_the_platform_suffixed_asset() -> TestResult {
    let (text, tree) = app()?;
    assert!(!text.contains("VERSION"));
    assert!(!text.contains("stage"));
    let publish = runs(job(&tree, "publish-app")?).join("\n");
    assert!(
        publish.contains("app-x86_64-unknown-linux-musl.tar.gz"),
        "{publish}"
    );
    Ok(())
}

#[test]
fn an_asset_name_with_version_and_target_names_every_file_after_the_tag_version() -> TestResult {
    let toml = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\nasset_name = \"{{ bin }}-{{ version }}-{{ target }}\"\n";
    let (_, tree) = render(&[APP], Some(toml))?;
    let linux = "app-${VERSION}-x86_64-unknown-linux-musl.tar.gz";
    let mac = "app-${VERSION}-aarch64-apple-darwin.tar.gz";

    let version = "echo \"VERSION=${TAG#app-v}\" >> \"$GITHUB_ENV\"";
    let build = job(&tree, "build-app-linux-x64")?;
    let build_runs = runs(build);
    assert!(build_runs.contains(&version), "{build_runs:?}");
    let pack =
        format!("tar -czf \"stage/{linux}\" -C target/x86_64-unknown-linux-musl/release app");
    assert!(build_runs.contains(&pack.as_str()), "{build_runs:?}");
    let reader = steps(build)
        .iter()
        .find(|step| step.get("run").and_then(Node::text) == Some(version))
        .ok_or("no version step")?;
    assert_eq!(
        reader.at(&["env", "TAG"]).and_then(Node::text),
        Some("${{ needs.tag.outputs.app }}")
    );
    let upload = steps(build)
        .last()
        .and_then(|step| step.at(&["with", "path"]))
        .and_then(Node::text);
    assert_eq!(upload, Some("stage/*"));

    let sums = runs(job(&tree, "sums-app")?);
    assert!(sums.contains(&version), "{sums:?}");
    let listed = format!("sha256sum \"{linux}\" \"{mac}\" > SHA256SUMS");
    assert!(sums.contains(&listed.as_str()), "{sums:?}");
    let publish = runs(job(&tree, "publish-app")?);
    assert!(publish.contains(&version), "{publish:?}");
    let command = publish.last().ok_or("no publish")?;
    assert!(
        command.ends_with(&format!("\"{linux}\" \"{mac}\"")),
        "{command}"
    );
    Ok(())
}

#[test]
fn an_asset_name_without_version_needs_no_tag_parsing() -> TestResult {
    let text = named("{{ bin }}_{{ platform }}", "\"linux-x64\"")?;
    assert!(text.contains("app_linux-x64.tar.gz"));
    assert!(!text.contains("VERSION"));
    assert!(!text.contains("stage"));
    assert!(!text.contains("app-linux-x64.tar.gz"));
    Ok(())
}

#[test]
fn an_asset_name_that_cannot_name_each_platform_or_is_unsafe_is_refused() -> TestResult {
    for template in [
        "{{ bin }}",
        "{{ bin }}-{{ platform }}-{{ tag }}",
        "{{ bin }}-{{ platform }}-{{version",
        "{{ bin }} {{ platform }}",
        "{{ bin }}/{{ platform }}",
        "$(id)-{{ platform }}",
        "",
    ] {
        let error = named(template, "\"linux-x64\", \"macos-arm64\"")
            .err()
            .ok_or_else(|| format!("{template:?} rendered"))?;
        assert!(
            matches!(
                error.downcast_ref::<GenerateError>(),
                Some(GenerateError::Release(
                    ReleaseError::InvalidAssetName { .. }
                ))
            ),
            "{template:?}: {error}"
        );
    }
    Ok(())
}
