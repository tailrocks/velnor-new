// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use std::collections::BTreeMap;
use velnor_ci::cargo_artifacts::{CargoProgress, ProgressKind};
use velnor_ci::cargo_package::PackageIdentity;
use velnor_ci::observed_units::{collect, diagnostics, normalize};
use velnor_ci::progress_evidence::{Evidence, corroborate};
use velnor_ci::telemetry::Invocation;

fn invocation() -> Invocation {
    let mut artifact = crate::cargo_artifacts::record(false);
    artifact["filenames"] = json!(["target/debug/deps/libapp-opaque.rlib"]);
    serde_json::from_value(json!({"argv":["cargo","build"],"root":".","configuration":"default","started_at":0,"seconds":0.0,"log_start":0,"log_end":0,"compilations":[],"downloads":[],"artifacts":[artifact]})).unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn dependency_source_identity_ignores_cargo_home_and_retains_package_source_and_version() {
    let packages = [
        "registry+https://registry.example/index#app@0.1.0",
        "registry+https://other.example/index#app@0.1.0",
        "registry+https://registry.example/index#other@0.1.0",
        "registry+https://registry.example/index#app@0.2.0",
        "git+https://git.example/repo?rev=abc#app@0.1.0",
        "git+https://git.example/other?rev=abc#app@0.1.0",
    ];
    let mut units = BTreeMap::new();
    for package in packages {
        for home in ["/agent/one/.cargo", "/agent/two/.cargo"] {
            let mut invocation = invocation();
            invocation
                .argv
                .push(format!("--manifest-path={home}/source/Cargo.toml"));
            let argv = invocation.argv.clone();
            let artifact = &mut invocation.artifacts[0];
            package.clone_into(&mut artifact.package_id);
            artifact.manifest_path = format!("{home}/source/Cargo.toml");
            artifact.target.src_path = format!("{home}/source/src/lib.rs");
            normalize(&mut invocation, std::path::Path::new("/checkout"));
            assert_eq!(invocation.argv, argv);
            assert_eq!(invocation.artifacts[0].package_id, package);
            assert_eq!(
                invocation.artifacts[0].target.src_path,
                "package/src/lib.rs"
            );
            collect(&invocation, &mut units);
        }
    }
    assert_eq!(units.len(), packages.len());
    assert!(units.values().all(|(_, count)| *count == 2));
    let mut different = invocation();
    packages[0].clone_into(&mut different.artifacts[0].package_id);
    different.artifacts[0].target.src_path = "/outside/src/lib.rs".to_owned();
    normalize(&mut different, std::path::Path::new("/checkout"));
    assert_eq!(
        different.artifacts[0].target.src_path,
        "/outside/src/lib.rs"
    );
    collect(&different, &mut units);
    assert_eq!(units.len(), packages.len() + 1);
}

#[test]
fn mixed_output_copies_preserve_the_qualified_compilation_identity() {
    let mut invocation = invocation();
    let mut units = BTreeMap::new();
    collect(&invocation, &mut units);
    invocation.artifacts[0]
        .filenames
        .push("target/debug/libapp.rlib".to_owned());
    collect(&invocation, &mut units);
    assert_eq!(units.len(), 1);
    assert_eq!(units.values().next().expect("unit").1, 2);
    invocation.artifacts[0].fresh = true;
    collect(&invocation, &mut units);
    assert_eq!(units.values().next().expect("unit").1, 2);
    for filename in ["target/debug/libapp.rlib", "target/debug/deps/libapp.rlib"] {
        invocation.artifacts[0].fresh = false;
        invocation.artifacts[0].filenames = vec![filename.to_owned()];
        let mut unknown = BTreeMap::new();
        collect(&invocation, &mut unknown);
        assert!(unknown.is_empty());
        assert_eq!(diagnostics(&invocation).len(), 1);
    }
    assert_checkout_normalization();
}

fn assert_checkout_normalization() {
    let mut units = BTreeMap::new();
    for (checkout, encoded) in [
        ("/agent one/工作", "/agent%20one/%E5%B7%A5%E4%BD%9C"),
        ("/agent two/工作", "/agent%20two/%E5%B7%A5%E4%BD%9C"),
    ] {
        let mut invocation = invocation();
        checkout.clone_into(&mut invocation.root);
        invocation
            .argv
            .push(format!("--manifest-path={checkout}/Cargo.toml"));
        let argv = invocation.argv.clone();
        invocation.target_directory = Some(format!("{checkout}/target"));
        let artifact = &mut invocation.artifacts[0];
        artifact.package_id = format!("path+file://{encoded}#app@0.1.0");
        artifact.manifest_path = format!("{checkout}/Cargo.toml");
        artifact.target.src_path = format!("{checkout}/src/lib.rs");
        artifact.filenames = vec![format!("{checkout}/target/debug/deps/libapp-opaque.rlib")];
        normalize(&mut invocation, std::path::Path::new(checkout));
        assert_eq!(invocation.root, ".");
        assert_eq!(invocation.argv, argv);
        assert_eq!(
            invocation.artifacts[0].package_id,
            "path+file://checkout#app@0.1.0"
        );
        collect(&invocation, &mut units);
    }
    assert_eq!(units.len(), 1);
    assert_eq!(
        units
            .values()
            .next()
            .unwrap_or_else(|| unreachable!("unit"))
            .1,
        2
    );
    for package in [
        "path+file:///agent%20one/%E5%B7%A5%E4%BD%9C-sibling#app@0.1.0",
        "path+file:///agent%Q0one/工作#app@0.1.0",
        "path+file:///agent%20one/%FF#app@0.1.0",
        "path+file:///agent%#app@0.1.0",
    ] {
        let mut invocation = invocation();
        "/agent one/工作-sibling".clone_into(&mut invocation.root);
        package.clone_into(&mut invocation.artifacts[0].package_id);
        normalize(&mut invocation, std::path::Path::new("/agent one/工作"));
        assert_eq!(invocation.root, "/agent one/工作-sibling");
        assert_eq!(invocation.artifacts[0].package_id, package);
    }
}

#[test]
fn canonical_documentation_outputs_identify_repeats_and_keep_purposes_distinct() {
    let mut invocation = invocation();
    invocation.artifacts[0].filenames = vec!["target/doc/app/index.html".to_owned()];
    let mut units = BTreeMap::new();
    for _ in 0..3 {
        collect(&invocation, &mut units);
    }
    assert_eq!(diagnostics(&invocation), Vec::<String>::new());
    assert_eq!(units.len(), 1);
    assert_eq!(units.values().next().expect("doc unit").1, 3);
    let mut features = invocation.clone();
    features.artifacts[0].features.push("extra".to_owned());
    collect(&features, &mut units);
    let mut profile = invocation.clone();
    profile.artifacts[0].profile.opt_level = "3".to_owned();
    collect(&profile, &mut units);
    invocation.artifacts[0].filenames = vec!["target/debug/deps/libapp-opaque.rmeta".to_owned()];
    collect(&invocation, &mut units);
    assert_eq!(units.len(), 4);
    assert_eq!(units.values().filter(|(_, count)| *count == 1).count(), 3);
}

#[test]
fn git_progress_matches_the_repository_and_explicit_revision_without_path_confusion() {
    assert_package_identity_forms();
    let mut invocation = invocation();
    invocation.artifacts[0].package_id = "git+file:///repo#app@0.1.0".to_owned();
    let mut event = CargoProgress {
        kind: ProgressKind::Compiling,
        package: "app".to_owned(),
        version: "0.1.0".to_owned(),
        location: Some("file:///repo#abcdef12".to_owned()),
    };
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("file:///other#abcdef12".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
    invocation.artifacts[0].package_id = "git+file:///repo?rev=abcdef123456#app@0.1.0".to_owned();
    event.location = Some("file:///repo?rev=abcdef123456#abcdef12".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("file:///repo?rev=abcdef123456#12345678".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
    invocation.artifacts[0].package_id = "path+file:///工作/app#0.1.0".to_owned();
    event.location = Some("/工作/app".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("/工作/other".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
}

fn assert_package_identity_forms() {
    assert_eq!(
        PackageIdentity::parse("unsupported?source=query#app@0.1.0")
            .map(|identity| identity.source),
        Some(Some("unsupported?source=query"))
    );
    assert_eq!(
        PackageIdentity::parse("app 0.1.0 (git+file:///repo?branch=main#abcdef12)")
            .map(|identity| identity.source),
        Some(Some("git+file:///repo?branch=main#abcdef12"))
    );
    assert_eq!(
        PackageIdentity::parse("app 0.1.0").map(|identity| identity.source),
        Some(None)
    );
    let mut invocation = invocation();
    "library_alias".clone_into(&mut invocation.artifacts[0].target.name);
    for package in [
        "path+file:///other-directory#app@0.1.0",
        "path+file:///app#0.1.0",
        "path+file:///app?source=query#0.1.0",
        "path+file:///app/?source=query#0.1.0",
        "unsupported#app@0.1.0",
        "app 0.1.0 (path+file:///app)",
        "app 0.1.0 (git+file:///repo#abcdef12)",
        "app 0.1.0",
    ] {
        package.clone_into(&mut invocation.artifacts[0].package_id);
        let mut event = CargoProgress {
            kind: ProgressKind::Compiling,
            package: "app".to_owned(),
            version: "0.1.0".to_owned(),
            location: Some("/工作/app".to_owned()),
        };
        invocation.artifacts[0].fresh = false;
        assert_eq!(
            corroborate(&invocation, &event),
            Evidence::Compiled,
            "{package}"
        );
        invocation.artifacts[0].fresh = true;
        assert_eq!(
            corroborate(&invocation, &event),
            Evidence::Fresh,
            "{package}"
        );
        "other".clone_into(&mut event.package);
        assert_eq!(
            corroborate(&invocation, &event),
            Evidence::Unavailable,
            "{package}"
        );
        "app".clone_into(&mut event.package);
        "9.0.0".clone_into(&mut event.version);
        assert_eq!(
            corroborate(&invocation, &event),
            Evidence::Unavailable,
            "{package}"
        );
        "0.1.0".clone_into(&mut event.version);
        event.location = Some("/elsewhere".to_owned());
        assert_eq!(
            corroborate(&invocation, &event),
            Evidence::Unavailable,
            "{package}"
        );
    }
}
