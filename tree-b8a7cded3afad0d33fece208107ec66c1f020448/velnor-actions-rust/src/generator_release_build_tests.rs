use super::{GeneratorBinaryCheck, GeneratorCargoBuild};
use velnor_actions_contract::{GeneratorReleasePlan, GeneratorReleaseTarget};

#[test]
fn every_target_build_uses_one_explicit_locked_release_command_and_output_root() {
    for target in GeneratorReleaseTarget::ALL {
        let build = GeneratorCargoBuild::new(target);
        assert_eq!(build.program(), "cargo");
        assert_eq!(
            build.args(),
            [
                "build",
                "--locked",
                "--release",
                "--package",
                "velnor-actions-cli",
                "--bin",
                "velnor-actions",
                "--target-dir",
                "target",
                "--target",
                target.triple(),
            ]
            .map(std::ffi::OsString::from)
        );
        assert_eq!(build.target(), target);
        assert_eq!(
            build.binary_relative_path(),
            std::path::PathBuf::from("target")
                .join(target.triple())
                .join("release")
                .join("velnor-actions")
        );
    }
}

#[test]
fn verification_binds_release_plan_target_output_and_closed_proofs() {
    let plan = GeneratorReleasePlan::for_version("0.1.1").expect("release plan");
    let cases = [
        (
            GeneratorReleaseTarget::LinuxX86_64,
            vec![
                GeneratorBinaryCheck::NativeHostIdentity,
                GeneratorBinaryCheck::RustToolchainIdentity,
                GeneratorBinaryCheck::BinaryFormatArchitecture,
                GeneratorBinaryCheck::GnuRuntimeAbi,
                GeneratorBinaryCheck::VersionSmoke,
                GeneratorBinaryCheck::HelpSmoke,
            ],
        ),
        (
            GeneratorReleaseTarget::MacosArm64,
            vec![
                GeneratorBinaryCheck::NativeHostIdentity,
                GeneratorBinaryCheck::RustToolchainIdentity,
                GeneratorBinaryCheck::BinaryFormatArchitecture,
                GeneratorBinaryCheck::AppleSdk,
                GeneratorBinaryCheck::AppleLinker,
                GeneratorBinaryCheck::VersionSmoke,
                GeneratorBinaryCheck::HelpSmoke,
            ],
        ),
    ];
    for (target, checks) in cases {
        let request = GeneratorCargoBuild::new(target)
            .verification(&plan, "1.98.1")
            .expect("verification request");
        assert_eq!(request.target(), target);
        assert_eq!(request.version(), "0.1.1");
        assert_eq!(request.rust_toolchain_version(), "1.98.1");
        assert_eq!(
            request.binary_relative_path(),
            GeneratorCargoBuild::new(target).binary_relative_path()
        );
        assert_eq!(request.checks(), checks);
    }
}

#[test]
fn verification_rejects_non_exact_rust_toolchain_version() {
    let plan = GeneratorReleasePlan::for_version("0.1.1").expect("release plan");
    let build = GeneratorCargoBuild::new(GeneratorReleaseTarget::LinuxX86_64);
    assert!(build.verification(&plan, "stable").is_err());
    assert!(build.verification(&plan, "1.98").is_err());
}
