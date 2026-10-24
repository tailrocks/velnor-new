use super::*;

#[test]
fn cold_export_records_absent_provider_root_in_the_isolated_v3_stage() {
    let fixture = Fixture::new();
    assert!(!fixture.cache_path.exists());

    let export = export_normal_tofu_providers_cache_payload(&fixture.request())
        .expect("cold ToFu payload export");

    assert_eq!(export.cache_key(), fixture.cache_key);
    assert!(export.state_digest().starts_with("b3-"));
    assert!(export.manifest_digest().starts_with("b3-"));
    assert_eq!(
        export.stage_path().file_name(),
        Some(std::ffi::OsStr::new("payload"))
    );
    let operation_directory = export.stage_path().parent().expect("operation directory");
    assert_eq!(
        operation_directory
            .parent()
            .and_then(std::path::Path::file_name),
        Some(std::ffi::OsStr::new("cache-staging"))
    );
    assert_eq!(
        operation_directory
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::file_name),
        Some(std::ffi::OsStr::new("velnor"))
    );

    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("ToFu action stage context");
    assert_eq!(export.stage_path(), context.action_path());
    let claim = crate::cache_action_stage::StageReceiptClaim::parse(export.stage_claim())
        .expect("canonical stage claim");
    let mut reader = crate::cache_action_stage::open_stage(&context, &claim)
        .expect("open the exact staged file");
    let mut transport = Vec::new();
    reader
        .read_to_end(&mut transport)
        .expect("read staged bytes");
    reader
        .revalidate()
        .expect("stage remains bound to its claim");

    assert!(transport.starts_with(b"velnor-cache-transport-v1\n"));
    assert!(
        transport
            .windows(b"normal_tofu_providers".len())
            .any(|window| window == b"normal_tofu_providers")
    );
    assert!(
        transport
            .windows(b"tofu_plugins".len())
            .any(|window| window == b"tofu_plugins")
    );
}

#[test]
fn export_binds_existing_provider_bytes_and_rewrites_only_the_claimed_stage() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.cache_path).expect("create provider cache root");
    fs::write(fixture.cache_path.join("provider.zip"), b"provider bytes")
        .expect("write provider archive");
    let old_context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let mut prior_writer =
        crate::cache_action_stage::create_stage(&old_context).expect("create a prior action stage");
    prior_writer
        .write_all(b"prior restore payload")
        .expect("write prior payload");
    let prior_claim = prior_writer.finish().expect("claim prior stage").encode();

    let mut request = fixture.request();
    request.previous_stage_claim = Some(&prior_claim);
    let export = export_normal_tofu_providers_cache_payload(&request)
        .expect("export providers over the claimed stage");

    assert_eq!(export.stage_path(), old_context.action_path());
    assert_ne!(export.stage_claim(), prior_claim);
    let claim = crate::cache_action_stage::StageReceiptClaim::parse(export.stage_claim())
        .expect("new stage claim");
    let mut reader = crate::cache_action_stage::open_stage(&old_context, &claim)
        .expect("open rewritten payload");
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .expect("read rewritten payload");
    reader.revalidate().expect("verify rewritten payload");
    assert!(bytes.starts_with(b"velnor-cache-transport-v1\n"));
    assert!(
        bytes
            .windows(b"provider.zip".len())
            .any(|window| window == b"provider.zip")
    );
    assert!(
        !bytes
            .windows(b"prior restore payload".len())
            .any(|window| window == b"prior restore payload")
    );
}

#[test]
fn preflight_reads_the_bounded_envelope_and_binds_its_manifest() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.cache_path.join("linux_amd64"))
        .expect("create provider-cache layout");
    fs::write(
        fixture.cache_path.join("linux_amd64/provider.zip"),
        b"checked provider archive bytes",
    )
    .expect("write provider archive");
    let export = export_normal_tofu_providers_cache_payload(&fixture.request())
        .expect("export valid cache payload");
    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("cache action context");
    let claim = crate::cache_action_stage::StageReceiptClaim::parse(export.stage_claim())
        .expect("stage claim");
    let mut reader = crate::cache_action_stage::open_stage(&context, &claim)
        .expect("open the exact transport file");
    let runtime = validate_request(&fixture.request()).expect("runtime binding");
    let roots = super::super::super::roots::CacheRootSet {
        matrix_key: "normal-tofu-providers".to_owned(),
        layer: velnor_actions_contract::QualificationCacheLayer::TofuProviders,
        roots: vec![super::super::super::roots::CacheRootObservation {
            id: super::super::super::roots::CacheRootId::TofuPlugins,
            path: fixture.cache_path.clone(),
        }],
    };
    let parsed = super::super::super::transfer::transport_tar_read::preflight(
        &mut reader,
        fixture.request().operation_limit_bytes,
        &roots,
        |text| {
            super::super::super::manifest::PayloadManifest::parse_for_normal_tofu_restore(
                text,
                &fixture.cache_key,
                &runtime,
                &roots,
            )
        },
    )
    .expect("validate tar inventory and manifest");
    reader.revalidate().expect("stage remains unchanged");

    assert_eq!(parsed.manifest.cache_key, fixture.cache_key);
    assert_eq!(parsed.entries.len(), 3);
    assert!(
        parsed
            .root_modes
            .contains_key(&super::super::super::roots::CacheRootId::TofuPlugins)
    );
    let file = parsed
        .entries
        .iter()
        .find(|entry| entry.path == "linux_amd64/provider.zip")
        .expect("provider file entry");
    let offset = file.data_offset.expect("regular member data offset");
    reader
        .seek(std::io::SeekFrom::Start(parsed.range.offset + offset))
        .expect("seek to member bytes");
    let mut bytes = vec![0; usize::try_from(file.size).expect("small fixture")];
    reader.read_exact(&mut bytes).expect("read member bytes");
    assert_eq!(bytes, b"checked provider archive bytes");
    assert!(
        parsed
            .entries
            .iter()
            .filter(|entry| entry.kind
                == super::super::super::transfer::transport_tar_read::MemberKind::Directory)
            .all(|entry| entry.data_offset.is_none())
    );
}
