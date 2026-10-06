use super::*;

fn collect(files: &[(&str, &str)]) -> Result<Vec<(String, String)>, OrchestratorError> {
    let root = tempfile::tempdir().expect("fixture");
    for (path, text) in files {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
        std::fs::write(path, text).expect("manifest");
    }
    Closure::new(root.path(), BTreeSet::new()).collect(BTreeSet::from(["Cargo.toml".to_owned()]))
}

#[test]
fn optional_target_workspace_and_patch_paths_are_exact_inputs() {
    let text = "[workspace]\n[workspace.dependencies]\na={path='a'}\n\
            [dependencies]\na={workspace=true}\n\
            [target.'cfg(windows)'.build-dependencies]\nb={path='b',optional=true}\n\
            [patch.crates-io]\nc={path='c'}\n";
    let inputs = collect(&[
        ("Cargo.toml", text),
        ("a/Cargo.toml", "[dependencies]\nb={path='../b'}\n"),
        ("b/Cargo.toml", "[dependencies]\na={path='../a'}\n"),
        ("c/Cargo.toml", "[package]\nname='c'\n"),
    ])
    .expect("contained cyclic closure");
    assert_eq!(inputs.len(), 4);
    assert_eq!(inputs[0], ("Cargo.toml".to_owned(), text.to_owned()));
}

#[test]
fn hidden_private_or_malformed_dependencies_fail_closed() {
    for body in [
        "[dev-dependencies]\nx={git='https://private'}",
        "[target.'cfg(no)'.dependencies]\nx={registry='private',version='1'}",
        "[workspace.dependencies]\nx={version='1',registry-index='https://private'}",
        "[patch.'https://private']\nx={path='x'}",
        "[replace]\n'x:1.0.0'={git='https://private'}",
        "[replace]\n'https://private/x#x:1.0.0'={version='1'}",
        "[dependencies]\nx=7",
        "[dependencies]\nx={version='1',unknown=true}",
        "[dependencies]\nx={workspace=true}",
        "[dependencies]\nx={path='../outside'}",
        "[dependencies]\nx={path='missing'}",
        "[dependencies]\nx={}",
        "[workspace]\nmembers=['unseen']",
    ] {
        assert!(collect(&[("Cargo.toml", body)]).is_err(), "{body}");
    }
}

#[test]
#[cfg(unix)]
fn symlink_manifest_and_external_directory_are_rejected() {
    let root = tempfile::tempdir().expect("fixture");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("Cargo.toml"), "[package]\nname='x'").expect("manifest");
    std::os::unix::fs::symlink(outside.path(), root.path().join("x")).expect("directory link");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[dependencies]\nx={path='x'}",
    )
    .expect("root");
    assert!(
        Closure::new(root.path(), BTreeSet::new())
            .collect(BTreeSet::from(["Cargo.toml".to_owned()]))
            .is_err()
    );
}

fn inputs(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut inputs: Vec<_> = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect();
    inputs.sort();
    inputs
}

#[test]
fn captured_closure_uses_same_policy_without_filesystem() {
    let roots = vec![String::new()];
    let good = inputs(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers=['a']\n\
        [workspace.dependencies]\nhelper={path='helper'}\n",
        ),
        (
            "a/Cargo.toml",
            "[package]\nname='a'\n[dependencies]\nhelper={workspace=true}\n",
        ),
        ("helper/Cargo.toml", "[package]\nname='helper'\n"),
    ]);
    assert!(qualify_captured(&roots, &good).is_ok());
    for body in [
        "[dependencies]\nx={path='missing',optional=true}",
        "[target.'cfg(windows)'.dependencies]\nx={git='https://private'}",
        "[workspace]\nmembers=['missing']",
        "[workspace]\nmembers=['crates/*']",
        "[workspace]\ndefault-members=['missing']",
    ] {
        assert!(
            qualify_captured(&roots, &inputs(&[("Cargo.toml", body)])).is_err(),
            "{body}"
        );
    }
    let mut duplicate = good.clone();
    duplicate.push(good[0].clone());
    assert!(qualify_captured(&roots, &duplicate).is_err());
    assert!(qualify_captured(&["missing".to_owned()], &good).is_err());
    assert!(qualify_captured(&roots, &inputs(&[("x/./Cargo.toml", "[package]")])).is_err());
    assert!(qualify_captured(&roots, &inputs(&[("Cargo.toml", &" ".repeat(64 * 1024))])).is_err());
}

#[test]
fn selected_package_requires_unique_exact_workspace_ownership() {
    let manifests = inputs(&[
        ("Cargo.toml", "[workspace]\nmembers=['a']"),
        ("a/Cargo.toml", "[package]\nname='a'"),
        ("other/Cargo.toml", "[workspace]\nmembers=['b']"),
        ("other/b/Cargo.toml", "[package]\nname='b'"),
    ]);
    assert!(selected_package(&manifests, "", "a").expect("a ownership"));
    assert!(!selected_package(&manifests, "", "b").expect("b ownership"));
    assert!(selected_package(&manifests, "other", "b").expect("b ownership"));
    assert!(!selected_package(&manifests, "", "missing").expect("missing"));
    let ambiguous = inputs(&[
        ("Cargo.toml", "[workspace]\nmembers=['a','b']"),
        ("a/Cargo.toml", "[package]\nname='same'"),
        ("b/Cargo.toml", "[package]\nname='same'"),
    ]);
    assert!(!selected_package(&ambiguous, "", "same").expect("ambiguous ownership"));
    let standalone = inputs(&[("Cargo.toml", "[package]\nname='app'")]);
    assert!(selected_package(&standalone, "", "app").expect("root ownership"));
}
