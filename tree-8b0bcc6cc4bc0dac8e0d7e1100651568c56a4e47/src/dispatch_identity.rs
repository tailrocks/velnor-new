//! Native dispatch observation; source authority comes from the external catalog.
//! This scope covers owned adapter children, never every process on the host.

#[cfg(feature = "owned-cache-transport")]
use crate::session::SessionDispatchPin;
use crate::session::completed_report::SessionIdentity;
use eyre::{Result, bail};
use mbx_cache_core::AdapterKind;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) const BEHAVIOR_ABI: &str = "mbx-owned-adapter-dispatch-v1";
pub(crate) const ADAPTER_ROUTES: [AdapterKind; 4] = [
    AdapterKind::Rustc,
    AdapterKind::Cc,
    AdapterKind::BuildScript,
    AdapterKind::Rustdoc,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MeasurementScope {
    MbxOwnedAdapters,
}

#[cfg(feature = "owned-cache-transport")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SnapshotPinning {
    VerifiedSessionSnapshots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RouteState {
    Managed,
    Disabled,
    ExternalWrapper,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RouteReason {
    DisabledByConfiguration,
    ExternalRustcWrapper,
    ExternalWorkspaceWrapper,
    ExternalRustcAndWorkspaceWrappers,
    DynamicInstallationPending,
    CompilerSelectionUnverified,
    DispatchNotVerified,
}

pub(crate) enum WrapperChain {
    Rustc,
    Workspace,
    RustcAndWorkspace,
}

pub(crate) enum UnknownReason {
    DynamicInstallationPending,
    CompilerSelectionUnverified,
    DispatchNotVerified,
}

/// These choices come from the owning session's final dispatch configuration.
/// A managed choice remains unverified until the constructor reads its shims.
pub(crate) struct RouteConfiguration {
    state: RouteState,
    shim_paths: Vec<PathBuf>,
    reason: Option<RouteReason>,
}

impl RouteConfiguration {
    pub(crate) fn managed(shim_paths: Vec<PathBuf>) -> Self {
        Self {
            state: RouteState::Managed,
            shim_paths,
            reason: None,
        }
    }

    pub(crate) fn disabled() -> Self {
        Self {
            state: RouteState::Disabled,
            shim_paths: Vec::new(),
            reason: Some(RouteReason::DisabledByConfiguration),
        }
    }

    pub(crate) fn external_wrapper(shim_paths: Vec<PathBuf>, chain: WrapperChain) -> Self {
        let reason = match chain {
            WrapperChain::Rustc => RouteReason::ExternalRustcWrapper,
            WrapperChain::Workspace => RouteReason::ExternalWorkspaceWrapper,
            WrapperChain::RustcAndWorkspace => RouteReason::ExternalRustcAndWorkspaceWrappers,
        };
        Self {
            state: RouteState::ExternalWrapper,
            shim_paths,
            reason: Some(reason),
        }
    }

    pub(crate) fn unknown(reason: UnknownReason) -> Self {
        let reason = match reason {
            UnknownReason::DynamicInstallationPending => RouteReason::DynamicInstallationPending,
            UnknownReason::CompilerSelectionUnverified => RouteReason::CompilerSelectionUnverified,
            UnknownReason::DispatchNotVerified => RouteReason::DispatchNotVerified,
        };
        Self {
            state: RouteState::Unknown,
            shim_paths: Vec::new(),
            reason: Some(reason),
        }
    }
}

/// Fixed adapter inventory; missing routes cannot silently become measured zero.
pub(crate) struct DispatchRoutes {
    pub rustc: RouteConfiguration,
    pub cc: RouteConfiguration,
    pub build_script: RouteConfiguration,
    pub rustdoc: RouteConfiguration,
}

#[derive(Debug, Clone, Serialize)]
struct ExecutableIdentity {
    canonical_path: PathBuf,
    sha256: String,
    actual_version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct RouteWitness {
    adapter: AdapterKind,
    state: RouteState,
    shim_paths: Vec<PathBuf>,
    executable_sha256: Option<String>,
    reason: Option<RouteReason>,
}

/// An observation, not a source authentication or host-wide process claim.
/// Only an external exact source/binary/ABI qualification can authorize reuse.
#[derive(Debug, Clone)]
pub(crate) struct NativeDispatchWitness {
    schema_version: u8,
    behavior_abi: &'static str,
    scope: MeasurementScope,
    session_id: String,
    root_session_id: String,
    actual_executable: ExecutableIdentity,
    source_base_version: &'static str,
    routes: [RouteWitness; 4],
    #[cfg(feature = "owned-cache-transport")]
    dispatch_pin: Option<SessionDispatchPin>,
}

impl NativeDispatchWitness {
    pub(crate) fn identity_matches(&self, session_id: &str, root_session_id: &str) -> bool {
        self.session_id == session_id && self.root_session_id == root_session_id
    }

    pub(crate) fn excluded_routes(&self) -> Vec<AdapterKind> {
        self.routes
            .iter()
            .filter(|route| route.state != RouteState::Managed)
            .map(|route| route.adapter)
            .collect()
    }

    pub(crate) fn behavior_abi(&self) -> &'static str {
        self.behavior_abi
    }

    #[cfg(feature = "owned-cache-transport")]
    pub(crate) fn bind_snapshot_pin(mut self, pin: &SessionDispatchPin) -> Result<Self> {
        self.validate_current()?;
        self.validate_pin(pin)?;
        self.dispatch_pin = Some(pin.clone());
        Ok(self)
    }

    pub(crate) fn snapshot_pinning_verified(&self) -> bool {
        #[cfg(feature = "owned-cache-transport")]
        {
            self.dispatch_pin
                .as_ref()
                .is_some_and(|pin| self.validate_pin(pin).is_ok())
        }
        #[cfg(not(feature = "owned-cache-transport"))]
        {
            false
        }
    }

    #[cfg(feature = "owned-cache-transport")]
    fn validate_pin(&self, pin: &SessionDispatchPin) -> Result<()> {
        if pin.owner_sha256() != self.actual_executable.sha256 {
            bail!("session snapshot pin belongs to another owning executable");
        }
        for route in &self.routes {
            for path in &route.shim_paths {
                pin.verify_route(path)?;
            }
        }
        Ok(())
    }

    /// An earlier observation cannot authorize a changed dispatch path.
    /// Pinned session routes additionally prevent ordinary upgrade drift.
    pub(crate) fn validate_current(&self) -> Result<()> {
        if executable_digest(&self.actual_executable.canonical_path)?
            != self.actual_executable.sha256
        {
            bail!("owning executable changed after dispatch verification");
        }
        for route in &self.routes {
            for path in &route.shim_paths {
                let expected = route.executable_sha256.as_ref().ok_or_else(|| {
                    eyre::eyre!("dispatch path lacks retained executable identity")
                })?;
                if executable_digest(path)? != *expected {
                    bail!(
                        "{:?} shim changed after dispatch verification",
                        route.adapter
                    );
                }
            }
        }
        #[cfg(feature = "owned-cache-transport")]
        if let Some(pin) = &self.dispatch_pin {
            self.validate_pin(pin)?;
        }
        Ok(())
    }

    pub(crate) fn verify(identity: &SessionIdentity, routes: DispatchRoutes) -> Result<Self> {
        if !cfg!(all(unix, feature = "owned-cache-transport")) {
            bail!("owned adapter dispatch witness requires the owned Unix source feature");
        }
        identity.validate()?;
        let executable = std::env::current_exe()?.canonicalize()?;
        Self::verify_at(identity, routes, &executable)
    }

    fn verify_at(
        identity: &SessionIdentity,
        routes: DispatchRoutes,
        executable: &Path,
    ) -> Result<Self> {
        let canonical_path = executable.canonicalize()?;
        let sha256 = executable_digest(&canonical_path)?;
        let configurations = [routes.rustc, routes.cc, routes.build_script, routes.rustdoc];
        let [rustc, cc, build_script, rustdoc] = configurations;
        let routes = [
            verify_route(ADAPTER_ROUTES[0], rustc, &sha256)?,
            verify_route(ADAPTER_ROUTES[1], cc, &sha256)?,
            verify_route(ADAPTER_ROUTES[2], build_script, &sha256)?,
            verify_route(ADAPTER_ROUTES[3], rustdoc, &sha256)?,
        ];
        if executable_digest(&canonical_path)? != sha256 {
            bail!("owning executable changed during dispatch verification");
        }
        Ok(Self {
            schema_version: 1,
            behavior_abi: BEHAVIOR_ABI,
            scope: MeasurementScope::MbxOwnedAdapters,
            session_id: identity.session_id.clone(),
            root_session_id: identity.root_session_id.clone(),
            actual_executable: ExecutableIdentity {
                canonical_path,
                sha256,
                actual_version: crate::version::VERSION,
            },
            source_base_version: crate::version::SOURCE_BASE_VERSION,
            routes,
            #[cfg(feature = "owned-cache-transport")]
            dispatch_pin: None,
        })
    }
}

impl Serialize for NativeDispatchWitness {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        self.validate_current().map_err(serde::ser::Error::custom)?;
        let mut output = serializer.serialize_struct("NativeDispatchWitness", 9)?;
        output.serialize_field("schema_version", &self.schema_version)?;
        output.serialize_field("behavior_abi", &self.behavior_abi)?;
        output.serialize_field("scope", &self.scope)?;
        output.serialize_field("session_id", &self.session_id)?;
        output.serialize_field("root_session_id", &self.root_session_id)?;
        output.serialize_field("actual_executable", &self.actual_executable)?;
        output.serialize_field("source_base_version", &self.source_base_version)?;
        output.serialize_field("routes", &self.routes)?;
        #[cfg(feature = "owned-cache-transport")]
        {
            let pinning = self
                .dispatch_pin
                .as_ref()
                .map(|_| SnapshotPinning::VerifiedSessionSnapshots);
            output.serialize_field("snapshot_pinning", &pinning)?;
        }
        #[cfg(not(feature = "owned-cache-transport"))]
        output.serialize_field("snapshot_pinning", &Option::<&str>::None)?;
        output.end()
    }
}

fn verify_route(
    adapter: AdapterKind,
    config: RouteConfiguration,
    expected: &str,
) -> Result<RouteWitness> {
    if config.state == RouteState::Managed && config.shim_paths.is_empty() {
        bail!("managed {adapter:?} dispatch requires an actual shim path");
    }
    let mut shim_paths = Vec::new();
    for shim in config.shim_paths {
        // Preserve the actual dispatch basename: several distinct shims may
        // legitimately resolve to the same owning executable.
        let name = shim
            .file_name()
            .ok_or_else(|| eyre::eyre!("shim path has no filename"))?;
        let absolute = std::path::absolute(&shim)?;
        let parent = absolute
            .parent()
            .ok_or_else(|| eyre::eyre!("shim path has no parent"))?;
        let installed_path = parent.canonicalize()?.join(name);
        let canonical = shim.canonicalize()?;
        if executable_digest(&canonical)? != expected {
            bail!("{adapter:?} dispatch shim differs from the owning executable");
        }
        if shim_paths.contains(&installed_path) {
            bail!("duplicate {adapter:?} dispatch shim path");
        }
        shim_paths.push(installed_path);
    }
    Ok(RouteWitness {
        adapter,
        state: config.state,
        executable_sha256: (!shim_paths.is_empty()).then(|| expected.to_owned()),
        shim_paths,
        reason: config.reason,
    })
}

fn executable_digest(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("dispatch executable is not a regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            bail!("dispatch file is not executable");
        }
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
#[path = "dispatch_identity_tests.rs"]
mod tests;
