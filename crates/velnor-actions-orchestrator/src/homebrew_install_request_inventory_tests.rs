use super::*;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use velnor_actions_contract::{
    CheckPlatform,
    homebrew_spec::{
        HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec, validate_homebrew_projection,
    },
    parse_package_release_plan,
};

fn fixture() -> (
    HomebrewFormulaSpec,
    velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    HomebrewFormulaReleaseLiterals,
    Vec<super::super::HomebrewInstallFile>,
) {
    let spec: HomebrewFormulaSpec = serde_json::from_slice(include_bytes!(
        "../../../.velnor/qualification/homebrew/formula-spec.json"
    ))
    .expect("pinned formula spec");
    let plan = parse_package_release_plan(include_bytes!(
        "../../../.velnor/qualification/homebrew/preview-plan.json"
    ))
    .expect("pinned preview plan");
    let projection = validate_homebrew_projection(&spec, &plan).expect("projection");
    let source_sha = "a".repeat(40);
    let version = "0.6.4-preview.1+aaaaaaa";
    let assets = projection
        .selected_payloads()
        .iter()
        .map(|payload| (payload.asset_name().to_owned(), "b".repeat(64)))
        .collect::<BTreeMap<_, _>>();
    let literals = HomebrewFormulaReleaseLiterals::new(
        &spec,
        &projection,
        source_sha.clone(),
        version.to_owned(),
        format!("preview-{source_sha}"),
        assets,
    )
    .expect("canonical release literals");
    let expected = crate::internal_plan::native_homebrew_checks::expected_install_files(
        &spec,
        &projection,
        CheckPlatform::MacosArm64,
    )
    .expect("host and auxiliary inventory");
    let files = expected
        .into_iter()
        .map(|file| super::super::HomebrewInstallFile {
            asset: file.asset,
            member: file.member,
            destination: file.destination,
            sha256: "c".repeat(64),
            mode: file.mode,
            test: file.test,
            elf: file
                .elf
                .map(|(class, endian, machine)| super::super::ElfFacts {
                    class,
                    endian,
                    machine,
                }),
        })
        .collect();
    (spec, projection, literals, files)
}

#[test]
fn host_files_match_brews_cleaned_mode_while_libexec_modes_stay_policy_bound() {
    let (spec, projection, literals, mut files) = fixture();
    let host_index = files
        .iter()
        .position(|file| file.destination.starts_with("bin/"))
        .expect("host executable");
    assert_eq!(files[host_index].mode, 0o555);
    assert_eq!(
        files
            .iter()
            .find(|file| file.destination.starts_with("libexec/"))
            .expect("Linux auxiliary")
            .mode,
        0o755
    );
    assert_eq!(
        super::super::auxiliary_installed_mode(
            velnor_actions_contract::homebrew_spec::HomebrewInstalledMode::Data
        ),
        0o644
    );
    assert_eq!(
        super::super::auxiliary_installed_mode(
            velnor_actions_contract::homebrew_spec::HomebrewInstalledMode::Executable
        ),
        0o755
    );
    let paths = super::super::HomebrewInstallPaths {
        home: PathBuf::from("/private/home"),
        temp: PathBuf::from("/private/tmp"),
        staging: PathBuf::from("/private/staging"),
    };
    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    {
        let request = HomebrewInstallRequest {
            spec: &spec,
            projection: &projection,
            literals: &literals,
            formula_bytes: b"",
            formula_sha256: "",
            files: &files,
            paths: &paths,
            deadline,
        };
        validate_file_inventory_for_host(&request, "aarch64-apple-darwin")
            .expect("555 Mac ARM host and policy auxiliary modes");
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        validate_file_inventory(&request).expect("production Mac ARM host inventory");
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        assert!(validate_file_inventory(&request).is_err());
    }
    files[host_index].mode = 0o755;
    let request = HomebrewInstallRequest {
        spec: &spec,
        projection: &projection,
        literals: &literals,
        formula_bytes: b"",
        formula_sha256: "",
        files: &files,
        paths: &paths,
        deadline,
    };
    assert!(validate_file_inventory_for_host(&request, "aarch64-apple-darwin").is_err());
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert!(validate_file_inventory(&request).is_err());
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    assert!(validate_file_inventory(&request).is_err());
}
