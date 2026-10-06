//! Immutable Swift wrapper emission and fixed stage calls.

use super::{
    GITHUB_SHA, NativeSwiftStage, SOURCE_ROOT_ENV, SOURCE_SHA_ENV, WORKFLOW_SHA_SENTINEL,
    invalid_contract, literal, sha256,
};
use velnor_actions_contract::ContractError;
use velnor_actions_rust::native_ffi::NativeFfiArtifacts;

pub(super) fn wrapper_body(
    bundle: &velnor_actions_native::SupportBundle,
    profile: &velnor_actions_native::OwnedSupportFile,
    stage: &NativeSwiftStage,
    source_root: &str,
    source_sha: &str,
    rust_artifacts: Option<&NativeFfiArtifacts>,
) -> Result<String, ContractError> {
    let modules = module_sources(bundle)?;
    let profile_source = literal(profile.source())?;
    let profile_digest = literal(&sha256(profile.source()))?;
    let source_root_name = literal(source_root)?;
    let source_sha_name = literal(if source_sha == GITHUB_SHA {
        WORKFLOW_SHA_SENTINEL
    } else {
        source_sha
    })?;
    let receipt = receipt_values(rust_artifacts)?;
    let call = stage_call(stage)?;
    Ok(format!(
        "set -euo pipefail\nexec python3 -I -S - <<'VELNOR_NATIVE_SWIFT'\nimport hashlib\nimport os\nimport sys\nimport types\n\nPROFILE_SOURCE = {profile_source}\nPROFILE_SHA256 = {profile_digest}\nEXPECTED_SOURCE_ROOT = {source_root_name}\nEXPECTED_SOURCE_SHA = {source_sha_name}\nRUST_ARTIFACTS = {receipt_path}\nRUST_PROFILE_DIGEST = {receipt_digest}\n\ndef preload(name, path, source):\n    module = types.ModuleType(name)\n    module.__file__ = '<velnor-inline:' + path + '>'\n    module.__package__ = ''\n    sys.modules[name] = module\n    exec(compile(source, module.__file__, 'exec'), module.__dict__)\n\nMODULES = {{\n{modules}}}\nfor name in ('desktop_native_core', 'desktop_native_build', 'desktop_native_verify',\n             'desktop_native_sign', 'desktop_native_state', 'desktop_native'):\n    preload(name, MODULES[name][0], MODULES[name][1])\n\nif hashlib.sha256(PROFILE_SOURCE.encode()).hexdigest() != PROFILE_SHA256:\n    raise SystemExit('native profile source digest mismatch')\nsource_root = os.environ.get('{source_root_env}')\nsource_sha = os.environ.get('{source_sha_env}')\nif source_root is None or source_sha is None:\n    raise SystemExit('native source authority missing')\nif source_root != EXPECTED_SOURCE_ROOT and EXPECTED_SOURCE_ROOT != '<workflow>':\n    raise SystemExit('native source root authority mismatch')\nif EXPECTED_SOURCE_SHA != '{github_sha}' and source_sha != EXPECTED_SOURCE_SHA:\n    raise SystemExit('native source SHA authority mismatch')\n\nfrom desktop_native_core import load_profile_source, safe_path, validate_source_sha\nfrom desktop_native import path\nprofile = load_profile_source(PROFILE_SOURCE, source_root)\nvalidate_source_sha(profile, source_sha)\nprofile['_source_sha'] = source_sha\nprofile['_rust_artifacts'] = safe_path(profile['_root'], RUST_ARTIFACTS) if RUST_ARTIFACTS else None\nprofile['_rust_profile_digest'] = RUST_PROFILE_DIGEST\nos.chdir(profile['_root'])\n{call}VELNOR_NATIVE_SWIFT\n",
        profile_digest = profile_digest,
        source_root_name = source_root_name,
        source_sha_name = source_sha_name,
        receipt_path = receipt.0,
        receipt_digest = receipt.1,
        modules = modules,
        source_root_env = SOURCE_ROOT_ENV,
        source_sha_env = SOURCE_SHA_ENV,
        github_sha = WORKFLOW_SHA_SENTINEL,
        call = call,
        profile_source = profile_source,
    ))
}

fn module_sources(bundle: &velnor_actions_native::SupportBundle) -> Result<String, ContractError> {
    let expected = velnor_actions_native::swift::desktop_helper_paths();
    if bundle.files().len() != expected.len()
        || bundle
            .files()
            .iter()
            .any(|file| !expected.contains(&file.path()))
    {
        return Err(invalid_contract("native_module_closure"));
    }
    let mut result = String::new();
    for file in bundle.files() {
        let module = file
            .path()
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".py"))
            .ok_or_else(|| invalid_contract("native_module_path"))?;
        result.push_str(&format!(
            "    {}: ({}, {}),\n",
            literal(module)?,
            literal(file.path())?,
            literal(file.source())?
        ));
    }
    Ok(result)
}

fn receipt_values(
    artifacts: Option<&NativeFfiArtifacts>,
) -> Result<(String, String), ContractError> {
    artifacts.map_or_else(
        || Ok(("None".to_owned(), "None".to_owned())),
        |value| {
            Ok((
                literal(value.record_path())?,
                literal(value.profile_digest())?,
            ))
        },
    )
}

fn stage_call(stage: &NativeSwiftStage) -> Result<String, ContractError> {
    let call = match stage {
        NativeSwiftStage::GenerateProject => "entry.generate_project(profile)\n".to_owned(),
        NativeSwiftStage::BindingsCheck => "entry.bindings_check(profile)\n".to_owned(),
        NativeSwiftStage::Xcframework => "entry.assemble_framework(profile)\n".to_owned(),
        NativeSwiftStage::Build(version, build) => format!(
            "entry.build_app(profile, {}, {})\n",
            literal(version)?,
            literal(build)?
        ),
        NativeSwiftStage::Verify(version, build, release, zip) => format!(
            "entry.verify_app(profile, {}, {}, release={}, zip_path={})\n",
            literal(version)?,
            literal(build)?,
            release,
            verify_zip_call(zip.as_deref())?
        ),
        NativeSwiftStage::SwiftBuild => "entry.run(['swift', 'build', '-c', 'release'], cwd=path(profile, 'native_root'), stream=True)\n".to_owned(),
        NativeSwiftStage::SwiftTest => "entry.swift_test(profile)\n".to_owned(),
        NativeSwiftStage::XcodeBuild => "entry.run(entry.xcode_arguments(profile) + ['ARCHS=arm64', 'CODE_SIGNING_ALLOWED=NO', 'MACOSX_DEPLOYMENT_TARGET=' + profile['deployment_target'], 'build'], cwd=path(profile, 'native_root'), stream=True)\n".to_owned(),
        NativeSwiftStage::XcodeTest => "entry.xcode_test(profile, ui=False)\n".to_owned(),
        NativeSwiftStage::UiTest => "entry.xcode_test(profile, ui=True)\n".to_owned(),
        NativeSwiftStage::Format => "entry.format_check(profile)\n".to_owned(),
        NativeSwiftStage::Lint => "entry.run(['swiftlint', 'lint', '--strict'] + (['--config', safe_path(path(profile, 'native_root'), profile['checks']['lint_config'])] if profile['checks'].get('lint_config') is not None else []), cwd=path(profile, 'native_root'))\n".to_owned(),
        NativeSwiftStage::Deadcode => "entry.deadcode(profile)\n".to_owned(),
        NativeSwiftStage::SwiftHarnesses => "entry.swift_harnesses(profile)\n".to_owned(),
        NativeSwiftStage::Sign(version, build) => format!(
            "desktop_native_sign.sign(profile, {}, {})\n",
            literal(version)?,
            literal(build)?
        ),
        NativeSwiftStage::State(version, repository) => format!(
            "desktop_native_state.release_state(profile, {}, {}, None)\n",
            literal(version)?,
            literal(repository)?
        ),
    };
    Ok(format!(
        "from desktop_native_core import path, safe_path\nimport desktop_native as entry\nimport desktop_native_sign\nimport desktop_native_state\n{call}",
        call = call
    ))
}

fn verify_zip_call(path: Option<&str>) -> Result<String, ContractError> {
    path.map_or_else(
        || Ok("None".to_owned()),
        |value| Ok(format!("safe_path(profile['_root'], {})", literal(value)?)),
    )
}
