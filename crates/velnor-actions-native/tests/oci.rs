//! Execute the complete offline OCI Python suite through its pinned tool owner.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_contract::ToolCacheDomain;
use velnor_actions_mise::catalog::qualification::DistributionHost;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ProcessOutput, RuntimePaths, ToolCatalog};

const SUITES: &[(&str, usize)] = &[
    ("oci_archive_test.py", 11),
    ("oci_index_receipt_test.py", 8),
    ("oci_platform_publish_test.py", 4),
    ("oci_registry_test.py", 9),
    ("oci_support_test.py", 6),
    ("oci_transport_test.py", 9),
];

fn distribution_host() -> Result<DistributionHost, Box<dyn std::error::Error>> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(DistributionHost::LinuxAmd64),
        ("linux", "aarch64") => Ok(DistributionHost::LinuxArm64),
        ("macos", "aarch64") => Ok(DistributionHost::MacosArm64),
        _ => Err("OCI suite requires an explicitly qualified Python host".into()),
    }
}

fn python(
    catalog: &ToolCatalog,
    host: DistributionHost,
    root: &Path,
    args: Vec<OsString>,
) -> Result<ProcessOutput, Box<dyn std::error::Error>> {
    // Archive qualification alone never grants installed executable authority.
    let launch = catalog.native_launch_context(host, ToolCacheDomain::Full, PinnedTool::Python)?;
    assert_eq!(launch.tool(), PinnedTool::Python);
    let output = PinnedToolExec::new(vec![PinnedTool::Python], OsStr::new("python3"), args)?
        .command_with_runtime_for_host(catalog, host, RuntimePaths::full())?
        .with_cwd(root.to_path_buf())
        .run_bounded(1_048_576, Duration::from_secs(120))?;
    output.require_success("offline OCI Python suite")?;
    Ok(output)
}

#[test]
fn all_offline_oci_python_tests_execute() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = ToolCatalog::pinned();
    let host = distribution_host()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
    let version = python(
        &catalog,
        host,
        &root,
        ["-I", "-S", "-B", "--version"].map(OsString::from).to_vec(),
    )?;
    assert_eq!(
        version.stdout_text("python3")?.trim(),
        format!("Python {}", catalog.version(PinnedTool::Python)),
    );
    for (name, count) in SUITES {
        let path = root.join("tests/oci").join(name);
        assert!(path.is_file(), "missing OCI suite: {}", path.display());
        let mut args = ["-I", "-S", "-B"].map(OsString::from).to_vec();
        args.push(path.into_os_string());
        let output = python(&catalog, host, &root, args)?;
        let stderr = std::str::from_utf8(&output.stderr)?;
        assert!(
            stderr
                .lines()
                .any(|line| line.starts_with(&format!("Ran {count} tests in "))),
            "{name} did not execute its complete test inventory: {stderr}",
        );
    }
    Ok(())
}
