// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::{ImageOpenssl, ImagePrBuild, ImageVisibility, RepoPath, RepoView};
use velnor_rust::detect;

use crate::facts::TestResult;
use crate::image_workflow::{edited, sample};

const MINIMAL: &str = "[[image]]\nname = \"plain-svc\"\nbins = [\"plain-svc\"]\n\n[[image.registry]]\nhost = \"docker.io\"\nrepository = \"acme/plain-svc\"\nusername = \"secrets.U\"\ntoken = \"secrets.T\"\n";

fn with_images(text: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    edited("image-plain", |config| {
        let head = config
            .split("[[image]]")
            .next()
            .unwrap_or_default()
            .to_owned();
        format!("{head}{text}")
    })
}

fn refusal(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(expected_files(&with_images(text)?)
        .err()
        .ok_or("the config was accepted")?
        .to_string())
}

#[test]
fn a_minimal_image_takes_every_default() -> TestResult {
    let view = with_images(MINIMAL)?;
    let facts = detect(&view)?;
    let [image] = facts.config.images.as_slice() else {
        return Err("one image expected".into());
    };
    assert_eq!(image.dockerfile, "Dockerfile");
    assert_eq!(
        (image.context.as_str(), image.stage.as_str()),
        (".", "dist")
    );
    assert_eq!(image.glibc, "2.41");
    assert_eq!(image.profile, "release");
    assert_eq!(image.openssl, ImageOpenssl::None);
    assert_eq!(image.pr_build, ImagePrBuild::Inputs);
    assert!(image.attestations && image.default_features);
    assert_eq!(image.timeout_minutes, 45);
    assert_eq!(image.registries[0].visibility, ImageVisibility::Private);
    Ok(())
}

#[test]
fn each_refused_value_names_its_key() -> TestResult {
    let base = ["name = \"plain-svc\"", "bins = [\"plain-svc\"]"];
    let registry = "[[image.registry]]\nhost = \"docker.io\"\nrepository = \"acme/plain-svc\"\nusername = \"secrets.U\"\ntoken = \"secrets.T\"\n";
    let cases = [
        ("name = \"Plain\"", "image.name"),
        ("name = \"x\"", "image.name"),
        ("bins = []", "image.bins"),
        ("bins = [\"a\", \"a\"]", "image.bins"),
        ("bins = [\"a/b\"]", "image.bins"),
        ("dockerfile = \"../Dockerfile\"", "image.dockerfile"),
        ("dockerfile = \"/etc/Dockerfile\"", "image.dockerfile"),
        ("context = \"a//b\"", "image.context"),
        ("stage = \".\"", "image.stage"),
        ("stage = \"dist/.git\"", "image.stage"),
        ("inputs = [\"../x\"]", "image.inputs"),
        ("context = \"app\"\ninputs = [\"other\"]", "image.inputs"),
        ("glibc = \"2.16\"", "image.glibc"),
        ("glibc = \"3.1\"", "image.glibc"),
        ("glibc = \"2.4x\"", "image.glibc"),
        ("openssl = \"static\"", "image.openssl"),
        ("pr_build = \"never\"", "image.pr_build"),
        ("profile = \"Release\"", "image.profile"),
        ("profile = \"bench\"", "image.profile"),
        ("features = [\"a,b\"]", "image.features"),
        ("package = \"a b\"", "image.package"),
        ("timeout_minutes = 0", "image.timeout_minutes"),
        ("pr_only_checks = [\"\"]", "image.pr_only_checks"),
        ("pr_only_checks = [\" DCO\"]", "image.pr_only_checks"),
        (
            "pr_only_checks = [\"DCO\", \"DCO\"]",
            "image.pr_only_checks",
        ),
    ];
    for (line, key) in cases {
        let overridden: Vec<&str> = line
            .lines()
            .filter_map(|text| text.split('=').next().map(str::trim))
            .collect();
        let kept: Vec<&str> = base
            .iter()
            .copied()
            .filter(|text| !overridden.iter().any(|name| text.starts_with(name)))
            .collect();
        let text = format!("[[image]]\n{}\n{line}\n\n{registry}", kept.join("\n"));
        let message = refusal(&text)?;
        assert!(message.contains(key), "{line}: {message}");
    }
    Ok(())
}

#[test]
fn each_refused_registry_names_its_key() -> TestResult {
    let image = "[[image]]\nname = \"plain-svc\"\nbins = [\"plain-svc\"]\n\n";
    let hub = "host = \"docker.io\"\nrepository = \"acme/plain-svc\"\n";
    let cases = [
        ("host = \"quay.io\"\nrepository = \"a/b\"\n".to_owned(), "image.registry.host"),
        (format!("{hub}token = \"secrets.T\"\n"), "image.registry.username"),
        (format!("{hub}username = \"secrets.U\"\n"), "image.registry.token"),
        (format!("{hub}username = \"secrets.U\"\ntoken = \"literal\"\n"), "image.registry.token"),
        (format!("{hub}username = \"secrets.U\"\ntoken = \"vars.T\"\n"), "image.registry.token"),
        (format!("{hub}username = \"secrets.GITHUB_X\"\ntoken = \"secrets.T\"\n"), "image.registry.username"),
        (format!("{hub}username = \"secrets.U\"\ntoken = \"secrets.T\"\nvisibility = \"hidden\"\n"), "image.registry.visibility"),
        ("host = \"docker.io\"\nrepository = \"Acme/x\"\nusername = \"secrets.U\"\ntoken = \"secrets.T\"\n".to_owned(), "image.registry.repository"),
        ("host = \"docker.io\"\nrepository = \"acme\"\nusername = \"secrets.U\"\ntoken = \"secrets.T\"\n".to_owned(), "image.registry.repository"),
        ("host = \"ghcr.io\"\nrepository = \"acme/x\"\ntoken = \"secrets.T\"\n".to_owned(), "image.registry.token"),
        ("host = \"ghcr.io\"\nrepository = \"acme/x\"\nusername = \"secrets.U\"\n".to_owned(), "image.registry.username"),
    ];
    for (registry, key) in cases {
        let text = format!("{image}[[image.registry]]\n{registry}");
        let message = refusal(&text)?;
        assert!(message.contains(key), "{registry}: {message}");
    }
    Ok(())
}

#[test]
fn the_set_of_images_and_registries_is_bounded_and_unique() -> TestResult {
    let two_hubs = format!(
        "{MINIMAL}\n[[image.registry]]\nhost = \"docker.io\"\nrepository = \"acme/b\"\nusername = \"secrets.U\"\ntoken = \"secrets.T\"\n"
    );
    assert!(refusal(&two_hubs)?.contains("image.registry.host"));
    let none = "[[image]]\nname = \"plain-svc\"\nbins = [\"plain-svc\"]\n";
    assert!(refusal(none)?.contains("image.registry"));
    let twice = format!("{MINIMAL}\n{MINIMAL}");
    assert!(refusal(&twice)?.contains("image.name"));
    let five: String = (0..5)
        .map(|index| {
            MINIMAL
                .replace(
                    "plain-svc\"]",
                    &format!("plain-svc\"]\nroot = \"r{index}\""),
                )
                .replace("name = \"plain-svc\"", &format!("name = \"svc-{index}\""))
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(refusal(&five)?.contains("image"));
    assert!(refusal(&MINIMAL.replace("bins =", "color = 1\nbins ="))?.contains("color"));
    Ok(())
}

#[test]
fn a_binary_the_repository_does_not_build_is_refused() -> TestResult {
    let view = edited("image-plain", |text| {
        text.replace("bins = [\"plain-svc\"]", "bins = [\"ghost\"]")
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::UnknownBin { .. }))
    ));
    let view = edited("image-workspace", |text| {
        text.replace("root = \"backend-rust\"", "root = \"elsewhere\"")
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::UnknownRoot { .. }))
    ));
    Ok(())
}

#[test]
fn openssl_none_is_refused_when_the_lock_holds_openssl_sys() -> TestResult {
    let view = edited("image-workspace", |text| {
        text.replace("openssl = \"sysroot\"\n", "")
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::OpensslRequired { .. }))
    ));
    let view = edited("image-workspace", |text| {
        text.replace("sysroot", "vendored")
    })?;
    assert!(expected_files(&view).is_ok());
    Ok(())
}

#[test]
fn a_tracked_file_under_the_stage_directory_is_refused() -> TestResult {
    let mut view = sample("image-plain")?;
    view.tracked_paths.insert(RepoPath::new("dist/plain-svc")?);
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::StageTracked { .. }))
    ));
    Ok(())
}

#[test]
fn zig_and_cargo_zigbuild_must_be_pinned() -> TestResult {
    for tool in [
        "zig = \"0.17.1\"\n",
        "\"aqua:rust-cross/cargo-zigbuild\" = \"0.24.1\"\n",
    ] {
        let mut view = sample("image-plain")?;
        let path = RepoPath::new("mise.toml")?;
        let text = String::from_utf8(view.files.remove(&path).ok_or("no mise.toml")?)?;
        view.files.insert(path, text.replace(tool, "").into_bytes());
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Image(ImageError::ToolMissing { .. }))
            ),
            "{tool}"
        );
    }
    Ok(())
}

#[test]
fn a_ghcr_namespace_must_be_the_owner_of_the_repository() -> TestResult {
    let mut view = sample("image-ghcr")?;
    view.repository = velnor_model::RepositoryIdentity::Named("acme/image-ghcr".to_owned());
    assert!(expected_files(&view).is_ok());
    view.repository = velnor_model::RepositoryIdentity::Named("other/image-ghcr".to_owned());
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::GhcrNamespace { .. }))
    ));
    view.repository = velnor_model::RepositoryIdentity::Named(String::new());
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::Unhosted))
    ));
    Ok(())
}

#[test]
fn an_image_needs_a_pin_that_reads_the_key_and_an_external_entry_cannot_name_its_workflow()
-> TestResult {
    let mut view = sample("image-plain")?;
    let lock = String::from_utf8(
        view.files
            .remove(&velnor_rust::LOCK_PATH)
            .ok_or("no lock")?,
    )?;
    view.files.insert(
        velnor_rust::LOCK_PATH,
        lock.replace("0.1.116", "0.1.115").into_bytes(),
    );
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::ConfigKeyHelperTooOld { key: "image", .. })
    ));
    let view = edited("image-plain", |text| {
        format!("external_workflows = [\".github/workflows/image.yml\"]\n{text}")
    })?;
    assert!(matches!(
        velnor_gen::external_skips(&view, &std::collections::BTreeMap::new()),
        Err(GenerateError::ExternalWorkflow(
            velnor_gen::ExternalWorkflowError::Collision(_)
        ))
    ));
    Ok(())
}

fn plain_with(
    change: impl FnOnce(String) -> String,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    edited("image-plain", change)
}

#[test]
fn two_images_cannot_share_a_registry_repository_or_a_release_tag_family() -> TestResult {
    let second = |name: &str, repository: &str| {
        format!(
            "\n[[image]]\nname = \"{name}\"\nbins = [\"plain-svc\"]\n\n[[image.registry]]\nhost = \"docker.io\"\nrepository = \"{repository}\"\nusername = \"secrets.DOCKERHUB_USERNAME\"\ntoken = \"secrets.DOCKERHUB_TOKEN\"\n"
        )
    };
    let shared = plain_with(|text| format!("{text}{}", second("other-svc", "acme/plain-svc")))?;
    assert!(matches!(
        expected_files(&shared),
        Err(GenerateError::Image(ImageError::RegistryShared { .. }))
    ));
    let overlapping = plain_with(|text| format!("{text}{}", second("plain-svc-v2", "acme/other")))?;
    assert!(matches!(
        expected_files(&overlapping),
        Err(GenerateError::Image(ImageError::TagFamilies { .. }))
    ));
    let distinct = plain_with(|text| format!("{text}{}", second("other-svc", "acme/other")))?;
    assert!(expected_files(&distinct).is_ok());
    Ok(())
}

#[test]
fn a_default_branch_that_is_not_an_image_tag_is_refused() -> TestResult {
    let view = plain_with(|text| {
        text.replace(
            "default_branch = \"main\"",
            "default_branch = \"release/stable\"",
        )
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::BranchTag { .. }))
    ));
    Ok(())
}

#[test]
fn the_sysroot_lock_and_a_custom_profile_must_exist() -> TestResult {
    let mut view = sample("image-workspace")?;
    let lock = RepoPath::new(".velnor/image-sysroot.lock")?;
    view.files.remove(&lock);
    view.tracked_paths.remove(&lock);
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::SysrootLockMissing { .. }))
    ));
    let view = plain_with(|text| {
        text.replace(
            "pr_build = \"always\"",
            "pr_build = \"always\"\nprofile = \"fast\"",
        )
    })?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::ProfileUndefined { .. }))
    ));
    let mut view = view;
    let manifest = RepoPath::new("Cargo.toml")?;
    let text = String::from_utf8(view.files.remove(&manifest).ok_or("no manifest")?)?;
    view.files.insert(
        manifest,
        format!("{text}\n[profile.fast]\ninherits = \"release\"\n").into_bytes(),
    );
    assert!(expected_files(&view).is_ok());
    Ok(())
}

#[test]
fn a_tool_pinned_only_for_another_operating_system_does_not_count() -> TestResult {
    let mut view = sample("image-plain")?;
    let path = RepoPath::new("mise.toml")?;
    let text = String::from_utf8(view.files.remove(&path).ok_or("no mise.toml")?)?;
    view.files.insert(
        path,
        text.replace(
            "zig = \"0.17.1\"",
            "zig = { version = \"0.17.1\", os = [\"macos\"] }",
        )
        .into_bytes(),
    );
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(ImageError::ToolMissing {
            tool: "zig",
            ..
        }))
    ));
    Ok(())
}
