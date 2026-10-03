use super::*;
use serde_json::{Value, json};

const UUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn request(args: &[&str]) -> Result<MetadataRequest, ObservationError> {
    let original = OriginalInvocation::new(
        "/bin/cargo".into(),
        args.iter().map(|value| (*value).into()).collect(),
        BTreeMap::new(),
        "/work".into(),
    )?;
    MetadataRequest::new(UUID.into(), UUID.into(), "nonce-1".into(), 1, original)
}

fn metadata() -> Value {
    json!({"version": 1, "workspace_root": "/work", "target_directory": "/work/target",
        "workspace_members": ["p"], "packages": [{"id":"p", "name":"p", "version":"0.1.0",
            "source":null, "manifest_path":"/work/Cargo.toml", "dependencies":[],
            "targets":[{"name":"p", "kind":["lib"], "crate_types":["lib"], "src_path":"/work/lib.rs"}]}],
        "resolve":{"root":"p", "nodes":[{"id":"p", "dependencies":[], "deps":[], "features":[]}]}})
}

fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or_default();
        let c = chunk.get(2).copied().unwrap_or_default();
        output.push(char::from(DIGITS[usize::from(a >> 2)]));
        output.push(char::from(DIGITS[usize::from(((a & 3) << 4) | (b >> 4))]));
        output.push(if chunk.len() > 1 {
            char::from(DIGITS[usize::from(((b & 15) << 2) | (c >> 6))])
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            char::from(DIGITS[usize::from(c & 63)])
        } else {
            '='
        });
    }
    output
}

fn envelope(request: &MetadataRequest, stdout: &[u8]) -> Result<Value, ObservationError> {
    Ok(
        json!({"request_binding": request.binding()?, "terminal":{"kind":"Exited", "code":0},
        "observed_wall_ns":12, "claimed_stdout_eof":true, "stdout_base64":encode(stdout)}),
    )
}

#[test]
fn leading_selector_and_valued_flags_are_preserved_without_false_features()
-> Result<(), ObservationError> {
    let request = request(&[
        "+nightly",
        "--config",
        "--all-features",
        "build",
        "-p",
        "--no-default-features",
        "-Freal",
        "--target=x86_64-unknown-linux-gnu",
    ])?;
    let projection = request.projection();
    assert_eq!(projection.toolchain_selector.as_deref(), Some("+nightly"));
    assert_eq!(
        projection.arguments,
        [
            "--config",
            "--all-features",
            "metadata",
            "--format-version",
            "1",
            "--features",
            "real",
            "--filter-platform",
            "x86_64-unknown-linux-gnu",
            "--locked",
            "--offline"
        ]
    );
    Ok(())
}

#[test]
fn unsupported_commands_flags_responses_and_missing_values_rejected() {
    for args in [
        vec!["install"],
        vec!["build", "--unknown"],
        vec!["build", "@response"],
        vec!["test", "--", "@response"],
        vec!["build", "--features"],
        vec!["build", "+nightly"],
    ] {
        assert!(request(&args).is_err(), "{args:?}");
    }
}

#[test]
fn request_projection_and_binding_are_exact() -> Result<(), ObservationError> {
    let request = request(&["build"])?;
    let bytes = serde_json::to_vec(&request).map_err(|_| ObservationError::InvalidJson)?;
    assert_eq!(parse_request(&bytes)?, request);
    let mut tampered: Value =
        serde_json::from_slice(&bytes).map_err(|_| ObservationError::InvalidJson)?;
    tampered["projection"]["arguments"][0] = json!("install");
    assert!(
        parse_request(&serde_json::to_vec(&tampered).map_err(|_| ObservationError::InvalidJson)?)
            .is_err()
    );
    let mut response = envelope(
        &request,
        &serde_json::to_vec(&metadata()).map_err(|_| ObservationError::InvalidJson)?,
    )?;
    response["request_binding"]["nonce"] = json!("nonce-2");
    assert!(matches!(
        parse_observation(
            &request,
            &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?
        ),
        Err(ObservationError::RequestMismatch)
    ));
    Ok(())
}

#[test]
fn complete_graph_stays_unqualified_and_source_is_not_guessed() -> Result<(), ObservationError> {
    let request = request(&["doc", "--no-deps"])?;
    assert!(
        !request
            .projection()
            .arguments
            .iter()
            .any(|arg| arg == "--no-deps")
    );
    let mut graph = metadata();
    graph["packages"][0]["source"] = json!("unfamiliar+opaque-source");
    let response = envelope(
        &request,
        &serde_json::to_vec(&graph).map_err(|_| ObservationError::InvalidJson)?,
    )?;
    let parsed = parse_observation(
        &request,
        &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?,
    )?;
    assert_eq!(parsed.status, UnqualifiedStatus::ParsedGraph);
    assert_eq!(
        parsed.reason,
        UnqualifiedReason::CargoResolutionNotQualified
    );
    assert_eq!(
        parsed.packages[0].source.as_deref(),
        Some("unfamiliar+opaque-source")
    );
    assert_eq!(parsed.packages[0].id, "p");
    assert_eq!(parsed.packages[0].manifest_path, "/work/Cargo.toml");
    assert_eq!(parsed.packages[0].target_sources, ["/work/lib.rs"]);
    assert!(parsed.packages[0].workspace_member);
    assert_eq!(parsed.terminal, TerminalObservation::Exited { code: 0 });
    assert_eq!(parsed.observed_wall_ns, Some(12));
    Ok(())
}

#[test]
fn duplicate_keys_rejected_at_every_json_level() {
    for bytes in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":{"y":1,"y":2}}"#,
        br#"[{"x":1,"\u0078":2}]"#,
    ] {
        assert!(strict_json::parse(bytes).is_err());
    }
}

#[test]
fn malformed_and_partial_graphs_rejected() -> Result<(), ObservationError> {
    let mut partial = metadata();
    partial["resolve"] = Value::Null;
    let mut wrong_version = metadata();
    wrong_version["version"] = json!(2);
    let mut dangling = metadata();
    dangling["resolve"]["nodes"][0]["dependencies"] = json!(["missing"]);
    let mut malformed_dependency = metadata();
    malformed_dependency["packages"][0]["dependencies"] = json!([{}]);
    let mut missing_node = metadata();
    missing_node["resolve"]["nodes"] = json!([]);
    for graph in [
        partial,
        wrong_version,
        dangling,
        malformed_dependency,
        missing_node,
    ] {
        assert!(
            graph::parse(&serde_json::to_vec(&graph).map_err(|_| ObservationError::InvalidJson)?)
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn strict_base64_rejects_noncanonical_padding_and_alphabet() -> Result<(), ObservationError> {
    for input in ["Zg==AAAA", "Zh==", "Zm9=", "Zg=", "Zg__", "Zg==\n", "===="] {
        assert!(base64::decode(input).is_err(), "{input}");
    }
    for bytes in [b"".as_slice(), b"f", b"fo", b"foo", &[0xff, 0, 0x80]] {
        assert_eq!(base64::decode(&encode(bytes))?, bytes);
    }
    Ok(())
}

#[test]
fn terminal_failure_and_eof_claim_cannot_supply_parsed_graph() -> Result<(), ObservationError> {
    let request = request(&["build"])?;
    let graph = serde_json::to_vec(&metadata()).map_err(|_| ObservationError::InvalidJson)?;
    for terminal in [
        json!({"kind":"Exited", "code":1}),
        json!({"kind":"Signaled", "signal":9}),
        json!({"kind":"WaitFailed"}),
        json!({"kind":"TimedOut"}),
    ] {
        let mut response = envelope(&request, &graph)?;
        response["terminal"] = terminal;
        let parsed = parse_observation(
            &request,
            &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?,
        )?;
        assert_eq!(parsed.status, UnqualifiedStatus::Unavailable);
        assert!(parsed.packages.is_empty());
    }
    let mut response = envelope(&request, &graph)?;
    response["claimed_stdout_eof"] = json!(false);
    let parsed = parse_observation(
        &request,
        &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?,
    )?;
    assert_eq!(parsed.status, UnqualifiedStatus::Unavailable);
    Ok(())
}

#[test]
fn inconsistent_wall_and_spawn_claims_rejected() -> Result<(), ObservationError> {
    let request = request(&["build"])?;
    let mut response = envelope(&request, b"")?;
    response["observed_wall_ns"] = Value::Null;
    assert!(
        parse_observation(
            &request,
            &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?
        )
        .is_err()
    );
    response["terminal"] = json!({"kind":"SpawnFailed"});
    response["claimed_stdout_eof"] = json!(false);
    assert_eq!(
        parse_observation(
            &request,
            &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?
        )?
        .status,
        UnqualifiedStatus::Unavailable
    );
    response["stdout_base64"] = json!("Zg==");
    assert!(
        parse_observation(
            &request,
            &serde_json::to_vec(&response).map_err(|_| ObservationError::InvalidJson)?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn bounds_reject_before_envelope_parse_and_invalid_identity() {
    assert!(matches!(
        parse_request(&vec![b' '; MAX_REQUEST_BYTES + 1]),
        Err(ObservationError::Bounds)
    ));
    assert!(
        MetadataRequest::new(
            UUID.into(),
            UUID.into(),
            "n".into(),
            3,
            OriginalInvocation {
                program: "/bin/cargo".into(),
                argv: vec!["build".into()],
                env: BTreeMap::new(),
                cwd: "/work".into()
            }
        )
        .is_err()
    );
}

#[test]
fn public_dependency_relationships_use_library_name_and_reject_missing_edge()
-> Result<(), ObservationError> {
    let declaration = json!({"name":"dep-package", "req":"*", "source":null,
        "kind":null, "target":null, "rename":null, "registry":null,
        "optional":false, "uses_default_features":true, "features":[]});
    let mut graph = metadata();
    graph["packages"][0]["dependencies"] = json!([declaration]);
    assert!(
        graph::parse(&serde_json::to_vec(&graph).map_err(|_| ObservationError::InvalidJson)?)
            .is_err()
    );
    let dependency = json!({"id":"dep", "name":"dep-package", "version":"1.0.0", "source":null,
        "manifest_path":"/dep/Cargo.toml", "dependencies":[], "targets":[{"name":"custom-library",
        "kind":["lib"], "crate_types":["lib"], "src_path":"/dep/lib.rs"}]});
    graph["packages"]
        .as_array_mut()
        .ok_or(ObservationError::InvalidGraph)?
        .push(dependency);
    graph["resolve"]["nodes"]
        .as_array_mut()
        .ok_or(ObservationError::InvalidGraph)?
        .push(json!({"id":"dep", "dependencies":[], "deps":[], "features":[]}));
    graph["resolve"]["nodes"][0]["dependencies"] = json!(["dep"]);
    graph["resolve"]["nodes"][0]["deps"] = json!([{"name":"custom_library", "pkg":"dep",
        "dep_kinds":[{"kind":null, "target":null}]}]);
    assert_eq!(
        graph::parse(&serde_json::to_vec(&graph).map_err(|_| ObservationError::InvalidJson)?)?
            .len(),
        2
    );
    graph["resolve"]["nodes"][0]["deps"][0]["name"] = json!("dep_package");
    assert!(
        graph::parse(&serde_json::to_vec(&graph).map_err(|_| ObservationError::InvalidJson)?)
            .is_err()
    );
    Ok(())
}
