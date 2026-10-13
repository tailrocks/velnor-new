use super::*;
use crate::session::completed_report::CommandRole;

fn identity() -> SessionIdentity {
    SessionIdentity::new(CommandRole::CargoBuild, None, None, None).unwrap()
}

fn managed(path: &Path) -> DispatchRoutes {
    DispatchRoutes {
        rustc: RouteConfiguration::managed(vec![path.into()]),
        cc: RouteConfiguration::managed(vec![path.into()]),
        build_script: RouteConfiguration::managed(vec![path.into()]),
        rustdoc: RouteConfiguration::managed(vec![path.into()]),
    }
}

#[cfg(unix)]
fn executable(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn verified_native_routes_bind_exact_bytes_identity_and_closed_inventory() {
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("mbx");
    let shim = temp.path().join("rustc");
    executable(&owner, b"source-qualified-owner");
    executable(&shim, b"source-qualified-owner");
    let identity = identity();
    let witness = NativeDispatchWitness::verify_at(&identity, managed(&shim), &owner).unwrap();
    assert_eq!(
        witness.routes.each_ref().map(|route| route.adapter),
        ADAPTER_ROUTES
    );
    assert!(
        witness
            .routes
            .iter()
            .all(|route| route.state == RouteState::Managed)
    );
    assert!(witness.validate_current().is_ok());
    assert!(witness.excluded_routes().is_empty());
    assert!(!witness.snapshot_pinning_verified());
    assert!(witness.identity_matches(&identity.session_id, &identity.root_session_id));
    assert!(!witness.identity_matches(&identity.root_session_id, "wrong-root"));
    assert_eq!(witness.behavior_abi(), BEHAVIOR_ABI);
    assert_eq!(
        witness.actual_executable.sha256,
        executable_digest(&owner).unwrap()
    );
    let value = serde_json::to_value(witness).unwrap();
    assert_eq!(value["scope"], "mbx_owned_adapters");
    assert_eq!(
        value["actual_executable"]["actual_version"],
        crate::version::VERSION
    );
    assert_eq!(value["routes"].as_array().unwrap().len(), 4);
    assert!(value.get("closed").is_none());
    assert_eq!(value["snapshot_pinning"], serde_json::Value::Null);
}

#[cfg(unix)]
#[test]
fn a_changed_or_nonexecutable_shim_cannot_mint_managed_witness() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("mbx");
    let shim = temp.path().join("rustc");
    executable(&owner, b"owner");
    executable(&shim, b"different");
    assert!(NativeDispatchWitness::verify_at(&identity(), managed(&shim), &owner).is_err());
    executable(&shim, b"owner");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(NativeDispatchWitness::verify_at(&identity(), managed(&shim), &owner).is_err());
}

#[test]
fn managed_route_requires_a_real_nonduplicated_path() {
    assert!(
        verify_route(
            AdapterKind::Rustc,
            RouteConfiguration::managed(vec![]),
            "hash"
        )
        .is_err()
    );
    let missing = PathBuf::from("/missing-owned-dispatch-test");
    assert!(
        verify_route(
            AdapterKind::Rustc,
            RouteConfiguration::managed(vec![missing]),
            "hash"
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn unsupported_routes_remain_explicit_and_never_prove_closure() {
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("mbx");
    executable(&owner, b"owner");
    let routes = DispatchRoutes {
        rustc: RouteConfiguration::external_wrapper(vec![owner.clone()], WrapperChain::Workspace),
        cc: RouteConfiguration::unknown(UnknownReason::CompilerSelectionUnverified),
        build_script: RouteConfiguration::unknown(UnknownReason::DynamicInstallationPending),
        rustdoc: RouteConfiguration::disabled(),
    };
    let witness = NativeDispatchWitness::verify_at(&identity(), routes, &owner).unwrap();
    assert_eq!(witness.excluded_routes(), ADAPTER_ROUTES);
    let value = serde_json::to_value(witness).unwrap();
    assert_eq!(value["routes"][0]["state"], "external_wrapper");
    assert_eq!(
        value["routes"][1]["reason"],
        "compiler_selection_unverified"
    );
    assert_eq!(value["routes"][2]["reason"], "dynamic_installation_pending");
    assert_eq!(value["routes"][3]["state"], "disabled");
}

#[cfg(unix)]
#[test]
fn distinct_dispatch_names_may_resolve_to_one_owner() {
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("mbx");
    let cc = temp.path().join("cc");
    let cxx = temp.path().join("c++");
    executable(&owner, b"owner");
    std::os::unix::fs::symlink(&owner, &cc).unwrap();
    std::os::unix::fs::symlink(&owner, &cxx).unwrap();
    let expected = executable_digest(&owner).unwrap();
    let route = verify_route(
        AdapterKind::Cc,
        RouteConfiguration::managed(vec![cc.clone(), cxx]),
        &expected,
    )
    .unwrap();
    assert_eq!(route.shim_paths.len(), 2);
    assert_eq!(route.shim_paths[0].file_name().unwrap(), "cc");
    assert!(
        verify_route(
            AdapterKind::Cc,
            RouteConfiguration::managed(vec![cc.clone(), cc]),
            &expected
        )
        .is_err()
    );
}

#[cfg(not(all(unix, feature = "owned-cache-transport")))]
#[test]
fn official_source_profile_cannot_mint_owned_dispatch_witness() {
    let owner = std::env::current_exe().unwrap();
    assert!(NativeDispatchWitness::verify(&identity(), managed(&owner)).is_err());
}

#[cfg(unix)]
#[test]
fn post_mint_replacement_invalidates_native_and_serialized_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("mbx");
    let shim = temp.path().join("rustc");
    executable(&owner, b"owner");
    executable(&shim, b"owner");
    let witness = NativeDispatchWitness::verify_at(&identity(), managed(&shim), &owner).unwrap();
    assert!(witness.validate_current().is_ok());
    executable(&shim, b"replaced-shim");
    assert!(witness.validate_current().is_err());
    assert!(serde_json::to_value(&witness).is_err());
    executable(&shim, b"owner");
    executable(&owner, b"replaced-owner");
    assert!(witness.validate_current().is_err());
    assert!(serde_json::to_value(&witness).is_err());
}
