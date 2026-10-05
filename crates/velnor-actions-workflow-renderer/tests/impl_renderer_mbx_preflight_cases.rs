use super::*;

#[test]
fn preflight_checks_exact_installs_before_exposing_paths() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let [preflight, action] = mbx_tool_steps(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
    )
    .expect("preflight steps");
    let StepKind::Shell { run, .. } = &preflight.kind else {
        panic!("preflight must be a shell step");
    };
    let script = run.get(2).expect("shell script");
    let StepKind::Action { env, .. } = &action.kind else {
        panic!("restore must be an action step");
    };
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    assert_eq!(
        env.get("MBX_CACHE_DIR").map(String::as_str),
        Some("${{ steps.mbx_cache_preflight.outputs.cache_dir }}")
    );
    assert_eq!(
        env.get("MBX_CACHE_EXPORT_GROUP").map(String::as_str),
        Some("${{ steps.mbx_cache_preflight.outputs.export_group }}")
    );
    assert_valid_preflight(&fixture, script)?;
    assert_rejects_unqualified_preflight(&fixture, script)?;
    Ok(())
}

fn assert_valid_preflight(fixture: &Fixture, script: &str) -> std::io::Result<()> {
    let valid = exact_probe_case();
    let (success, paths, stderr) = fixture.run(script, valid)?;
    assert!(success, "exact installs pass: {paths:?}; stderr: {stderr}");
    assert_eq!(
        paths,
        format!(
            "{}\n{}\n",
            fixture.rust_root.display(),
            fixture.mbx_root.display()
        )
    );
    assert_eq!(
        fixture.visible_tools(&paths)?,
        [
            fixture.mbx_bin.display().to_string(),
            fixture.rustc_bin.display().to_string()
        ],
        "the action resolves the same validated MBX binary and Rustup shim"
    );
    let output = fs::read_to_string(&fixture.github_output)?;
    let root = fixture
        .root
        .0
        .join("velnor/mbx-job-741852963-12001-1-mbx_lifecycle");
    let transport_parent = fixture.workspace.join(".velnor-mbx-cache");
    let transport_lease = fixture.workspace.join(".velnor-mbx-cache.lock");
    for (key, expected) in [
        ("workspace_root", fixture.workspace.display().to_string()),
        ("transport_parent", transport_parent.display().to_string()),
        ("transport_lease", transport_lease.display().to_string()),
        ("job_root", root.display().to_string()),
        ("job_identity", "741852963:12001:1:mbx_lifecycle".to_owned()),
        ("cache_dir", root.join("store").display().to_string()),
        (
            "cold_cache_dir",
            root.join("store-cold").display().to_string(),
        ),
        (
            "bundle_path",
            transport_parent
                .join("mbx-single-bundle")
                .display()
                .to_string(),
        ),
        ("import_cwd", root.join("import").display().to_string()),
        (
            "export_group",
            "velnor-741852963-12001-1-mbx_lifecycle".to_owned(),
        ),
    ] {
        assert!(
            output
                .lines()
                .any(|line| line == format!("{key}={expected}")),
            "preflight output contains {key}: {output}"
        );
    }
    for path in [
        root.clone(),
        root.join("store"),
        root.join("store-cold"),
        root.join("import"),
        transport_parent.clone(),
        transport_lease,
        transport_parent.join("mbx-single-bundle"),
    ] {
        assert_eq!(
            fs::metadata(&path)?.permissions().mode() & 0o777,
            0o700,
            "preflight creates private directory {}",
            path.display()
        );
    }
    let env = fs::read_to_string(&fixture.github_env)?;
    assert!(env.contains(&format!("MBX_CACHE_DIR={}\n", root.join("store").display())));
    assert!(env.contains("MBX_CACHE_EXPORT_GROUP=velnor-741852963-12001-1-mbx_lifecycle\n"));
    Ok(())
}

fn assert_rejects_unqualified_preflight(fixture: &Fixture, script: &str) -> std::io::Result<()> {
    let valid = exact_probe_case();
    let cases = [
        ProbeCase {
            name: "missing MBX",
            mbx_version: None,
            ..valid
        },
        ProbeCase {
            name: "non-executable MBX",
            mbx_executable: false,
            ..valid
        },
        ProbeCase {
            name: "wrong MBX version",
            mbx_version: Some("1.20.0"),
            ..valid
        },
        ProbeCase {
            name: "missing named Rust toolchain",
            rust_mode: "missing-toolchain",
            ..valid
        },
        ProbeCase {
            name: "wrong Rust toolchain",
            rust_mode: "wrong-toolchain",
            ..valid
        },
        ProbeCase {
            name: "relative MBX install path",
            mise_mode: "relative-mbx",
            ..valid
        },
        ProbeCase {
            name: "relative Rust install path",
            mise_mode: "relative-rust",
            ..valid
        },
        ProbeCase {
            name: "missing Rustup shim",
            rustc_present: false,
            ..valid
        },
        ProbeCase {
            name: "non-executable Rustup shim",
            rustc_executable: false,
            ..valid
        },
    ];
    for case in cases {
        assert_rejected(fixture, script, case)?;
    }
    Ok(())
}

pub(super) fn exact_probe_case() -> ProbeCase {
    ProbeCase {
        name: "exact installs",
        mbx_version: Some(TEST_MBX_VERSION),
        mbx_executable: true,
        mise_mode: "absolute",
        rust_mode: "ok",
        rustc_present: true,
        rustc_executable: true,
    }
}
