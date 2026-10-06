//! Closed Rust producer recipes and artifact prerequisites for native consumers.

use crate::CompileDriver;
use serde::Serialize;
use velnor_actions_contract::config::RustFfiProfile;
use velnor_actions_contract::{
    ContractError, canonical::digest_b3, generated_source, normalize_posix_path,
};

pub use crate::native_ffi_evidence::{native_ffi_required_inputs, validate_native_ffi_evidence};

const SOURCE: &str = include_str!("native_ffi.py");

/// A fixed Rust producer operation selected during workflow generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFfiKind {
    /// Regenerate bindings without compiling the static library.
    Bindings,
    /// Compile the static library and regenerate its foreign bindings.
    Library,
}

/// Validated prerequisite locations relative to the admitted source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFfiArtifacts {
    output_root: String,
    record_path: String,
    bindings_path: String,
    library_path: Option<String>,
    profile_digest: String,
}

impl NativeFfiArtifacts {
    /// Complete atomic producer directory.
    #[must_use]
    pub fn output_root(&self) -> &str {
        &self.output_root
    }
    /// Typed descriptor consumed by Swift; binds every produced file digest.
    #[must_use]
    pub fn record_path(&self) -> &str {
        &self.record_path
    }
    /// Generated foreign-language binding directory.
    #[must_use]
    pub fn bindings_path(&self) -> &str {
        &self.bindings_path
    }
    /// Static library prerequisite, absent for bindings-only production.
    #[must_use]
    pub fn library_path(&self) -> Option<&str> {
        self.library_path.as_deref()
    }
    /// Exact Rust producer configuration identity checked by consumers.
    #[must_use]
    pub fn profile_digest(&self) -> &str {
        &self.profile_digest
    }
}

/// Typed Rust tool requirements lowered exclusively by the Mise adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRustToolRequirements {
    driver: CompileDriver,
    boltffi: bool,
    nextest: bool,
    target: Option<&'static str>,
}

impl NativeRustToolRequirements {
    /// Selected Rust compile owner; never selected by process environment.
    #[must_use]
    pub const fn compile_driver(&self) -> CompileDriver {
        self.driver
    }
    /// Whether the fixed producer requires `BoltFFI`.
    #[must_use]
    pub const fn boltffi(&self) -> bool {
        self.boltffi
    }
    /// Whether fixed library tests require Nextest.
    #[must_use]
    pub const fn nextest(&self) -> bool {
        self.nextest
    }
    /// Required Rust compilation target, separate from tool catalog pins.
    #[must_use]
    pub const fn target(&self) -> Option<&'static str> {
        self.target
    }
}

/// Fixed owner source; no user configuration supplies source, argv or dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRustProgram {
    path: String,
    source: String,
    tools: NativeRustToolRequirements,
}

impl NativeRustProgram {
    /// Generator-owned generated source destination.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Complete compiled source with generation-time inputs embedded.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Typed prerequisites for the SDK's fixed native tool role.
    #[must_use]
    pub const fn tool_requirements(&self) -> &NativeRustToolRequirements {
        &self.tools
    }
    /// Exact isolated Python invocation before Mise lowering.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        vec!["python3".to_owned(), "-I".to_owned(), self.path.clone()]
    }
}

/// Rust-owned producer program and typed prerequisites for native assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFfiProposal {
    program: NativeRustProgram,
    artifacts: NativeFfiArtifacts,
}

impl NativeFfiProposal {
    /// Owner-qualified fixed producer source.
    #[must_use]
    pub const fn program(&self) -> &NativeRustProgram {
        &self.program
    }
    /// Native consumer prerequisite locations and configuration identity.
    #[must_use]
    pub const fn artifacts(&self) -> &NativeFfiArtifacts {
        &self.artifacts
    }
}

#[derive(Serialize)]
struct ProducerRequest<'a> {
    source_root: &'a str,
    source_sha: &'a str,
    compile_driver: &'a str,
    manifest_path: &'a str,
    package: &'a str,
    profile: &'a str,
    features: &'a [String],
    static_library: &'a str,
    framework_name: &'a str,
    module_name: &'a str,
    deployment_target: &'a str,
    output_root: &'a str,
    profile_digest: &'a str,
}

/// Derive the complete source-bound Rust FFI prerequisite from typed identities.
/// # Errors
/// Rejects unsafe identities, unqualified source SHA, paths or marker versions.
pub fn native_ffi_proposal(
    ffi: &RustFfiProfile,
    deployment_target: &str,
    source_root: &str,
    source_sha: &str,
    driver: CompileDriver,
    kind: NativeFfiKind,
    version: &str,
) -> Result<NativeFfiProposal, ContractError> {
    ffi.validate(".velnor/config.toml", "native.ffi")?;
    validate_source(source_root, source_sha)?;
    validate_deployment(deployment_target)?;
    let artifacts = artifacts(ffi, deployment_target, kind)?;
    let request = ProducerRequest {
        source_root,
        source_sha,
        compile_driver: driver.as_str(),
        manifest_path: &ffi.manifest_path,
        package: &ffi.package,
        profile: &ffi.profile,
        features: &ffi.features,
        static_library: &ffi.static_library,
        framework_name: &ffi.framework_name,
        module_name: &ffi.module_name,
        deployment_target,
        output_root: &artifacts.output_root,
        profile_digest: &artifacts.profile_digest,
    };
    let call = if kind == NativeFfiKind::Library {
        "produce(REQUEST, build=True)"
    } else {
        "produce(REQUEST, build=False)"
    };
    Ok(NativeFfiProposal {
        program: program(
            &request,
            call,
            version,
            NativeRustToolRequirements {
                driver,
                boltffi: true,
                nextest: false,
                target: Some("aarch64-apple-darwin"),
            },
        )?,
        artifacts,
    })
}

fn artifacts(
    ffi: &RustFfiProfile,
    deployment: &str,
    kind: NativeFfiKind,
) -> Result<NativeFfiArtifacts, ContractError> {
    let parent = ffi
        .xcframework_path
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let output_root = if parent.is_empty() {
        format!(".velnor-rust-ffi/{}", ffi.package)
    } else {
        format!("{parent}/.velnor-rust-ffi/{}", ffi.package)
    };
    let identity = velnor_actions_contract::canonical::canonical_json_bytes(&(ffi, deployment))?;
    Ok(NativeFfiArtifacts {
        record_path: format!("{output_root}/artifacts.json"),
        bindings_path: format!("{output_root}/bindings"),
        library_path: (kind == NativeFfiKind::Library)
            .then(|| format!("{output_root}/library/{}", ffi.static_library)),
        output_root,
        profile_digest: digest_b3(&identity),
    })
}

#[derive(Serialize)]
struct TestRequest<'a> {
    source_root: &'a str,
    source_sha: &'a str,
    compile_driver: &'a str,
    packages: &'a [String],
}

/// Produce fixed locked library-test execution with explicit package selection.
/// # Errors
/// Rejects missing, duplicated, unsorted or unsafe packages/source identities.
pub fn native_library_tests(
    packages: &[String],
    source_root: &str,
    source_sha: &str,
    driver: CompileDriver,
    version: &str,
) -> Result<NativeRustProgram, ContractError> {
    validate_source(source_root, source_sha)?;
    if packages.is_empty()
        || packages.len() > 128
        || !packages.windows(2).all(|pair| pair[0] < pair[1])
        || packages.iter().any(|package| !valid_package(package))
    {
        return Err(failure("invalid_native_test_packages"));
    }
    program(
        &TestRequest {
            source_root,
            source_sha,
            compile_driver: driver.as_str(),
            packages,
        },
        "library_tests(REQUEST)",
        version,
        NativeRustToolRequirements {
            driver,
            boltffi: false,
            nextest: true,
            target: None,
        },
    )
}

fn program<T: Serialize>(
    request: &T,
    call: &str,
    version: &str,
    tools: NativeRustToolRequirements,
) -> Result<NativeRustProgram, ContractError> {
    let data = serde_json::to_string(request).map_err(|error| failure(&error.to_string()))?;
    let literal = serde_json::to_string(&data).map_err(|error| failure(&error.to_string()))?;
    let body = format!("{SOURCE}\nREQUEST = json.loads({literal})\n{call}\n");
    let source = generated_source(version, &body)?;
    let digest = digest_b3(source.as_bytes());
    Ok(NativeRustProgram {
        path: format!(
            ".github/velnor/rust/native/{}.py",
            digest.trim_start_matches("b3-")
        ),
        source,
        tools,
    })
}

fn validate_source(root: &str, sha: &str) -> Result<(), ContractError> {
    if (root != "." && normalize_posix_path(root)? != root)
        || sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(failure("invalid_native_source_identity"));
    }
    Ok(())
}

fn validate_deployment(value: &str) -> Result<(), ContractError> {
    let parts: Vec<_> = value.split('.').collect();
    if value.len() > 16
        || parts.len() != 2
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(failure("invalid_native_deployment_target"));
    }
    Ok(())
}

fn valid_package(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn failure(reason: &str) -> ContractError {
    ContractError::identity("rust_native", reason)
}

#[cfg(test)]
#[path = "native_ffi_tests.rs"]
mod tests;
