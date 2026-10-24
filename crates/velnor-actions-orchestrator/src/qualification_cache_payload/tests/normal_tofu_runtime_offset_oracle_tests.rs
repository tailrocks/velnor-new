use super::Fixture;
use std::io::{Read, Seek, SeekFrom};

#[test]
fn exported_provider_bytes_round_trip_from_the_claimed_cache_stage() {
    const PROVIDER_BYTES: &[u8] = b"checked provider bytes";

    let fixture = Fixture::new();
    let provider_root = fixture.cache_path.join("linux_amd64");
    std::fs::create_dir_all(&provider_root).expect("create provider root");
    std::fs::write(provider_root.join("provider.zip"), PROVIDER_BYTES)
        .expect("write provider bytes");

    let request = fixture.request();
    let export = super::export_normal_tofu_providers_cache_payload(&request)
        .expect("export provider cache stage");
    let mut reader = preflight_exported_stage(&fixture, &request, &export);

    super::super::super::transfer::validate_transport_envelope(
        &mut reader,
        request.operation_limit_bytes,
    )
    .expect("validate bounded transport envelope");
    let tar_start = reader.stream_position().expect("raw tar start");
    let stage_end = reader.seek(SeekFrom::End(0)).expect("stage end");
    let tar_length = stage_end
        .checked_sub(tar_start)
        .expect("raw tar range starts within the stage");
    let cache_root_name = super::super::super::transfer::root_name(
        super::super::super::roots::CacheRootId::TofuPlugins,
    );
    let expected_root = format!("roots/{cache_root_name}");
    let expected_member = format!("{expected_root}/linux_amd64/provider.zip");
    let (payload_offset, payload_size) = locate_regular_member(
        &mut reader,
        tar_start,
        tar_length,
        &expected_root,
        &expected_member,
    );

    assert_eq!(
        payload_size,
        u64::try_from(PROVIDER_BYTES.len()).expect("small fixture")
    );
    let relative_end = payload_offset
        .checked_add(payload_size)
        .expect("provider range end");
    assert!(
        relative_end <= tar_length,
        "provider bytes stay in the raw tar range"
    );
    let absolute_offset = tar_start
        .checked_add(payload_offset)
        .expect("absolute provider offset");
    let absolute_end = absolute_offset
        .checked_add(payload_size)
        .expect("absolute provider range end");
    assert!(
        absolute_end <= stage_end,
        "provider bytes stay in the claimed stage"
    );

    reader
        .seek(SeekFrom::Start(absolute_offset))
        .expect("seek to provider member bytes");
    let mut bytes = vec![0; PROVIDER_BYTES.len()];
    reader
        .read_exact(&mut bytes)
        .expect("read provider member bytes");
    assert_eq!(bytes.as_slice(), PROVIDER_BYTES);
    reader
        .revalidate()
        .expect("stage remains bound to its claim");
}

fn preflight_exported_stage(
    fixture: &Fixture,
    request: &super::NormalTofuProvidersCachePayloadRequest<'_>,
    export: &super::NormalTofuProvidersCacheExport,
) -> crate::cache_action_stage::StageReader {
    let runtime = super::validate_request(request).expect("validate runtime");
    let roots = super::super::super::roots::CacheRootSet {
        owner_id: "normal-tofu-providers".to_owned(),
        layer: velnor_actions_contract::QualificationCacheLayer::TofuProviders,
        roots: vec![super::super::super::roots::CacheRootObservation {
            id: super::super::super::roots::CacheRootId::TofuPlugins,
            path: fixture.cache_path.clone(),
        }],
    };
    let context = crate::cache_action_stage::StageContext::normal_tofu_providers(
        &fixture.runner_temp,
        &fixture.workspace,
        &fixture.cache_key,
    )
    .expect("stage context");
    let claim = crate::cache_action_stage::StageReceiptClaim::parse(export.stage_claim())
        .expect("claim exported stage");
    let mut reader =
        crate::cache_action_stage::open_stage(&context, &claim).expect("open the claimed stage");
    let manifest = super::super::super::transfer::transport_tar_read::preflight(
        &mut reader,
        request.operation_limit_bytes,
        &roots,
        |text| {
            super::super::super::manifest::PayloadManifest::parse_for_normal_tofu_restore(
                text,
                request.cache_key,
                &runtime,
                &roots,
            )
        },
    )
    .expect("preflight bound stage and manifest");
    assert_eq!(manifest.state_digest.as_str(), export.state_digest());
    reader
}

fn locate_regular_member<R>(
    reader: &mut R,
    tar_start: u64,
    tar_length: u64,
    expected_root: &str,
    expected_member: &str,
) -> (u64, u64)
where
    R: Read + Seek,
{
    reader
        .seek(SeekFrom::Start(tar_start))
        .expect("seek to bounded raw tar");
    let (roots_directory_seen, cache_root_seen, member) = {
        let mut archive = tar::Archive::new((&mut *reader).take(tar_length));
        let mut roots_directory_seen = false;
        let mut cache_root_seen = false;
        let mut member = None;

        for item in archive.entries().expect("read tar entries") {
            let entry = item.expect("read tar entry");
            let path = entry.path().expect("validated tar path");
            let path = path.to_str().expect("fixture paths are UTF-8");
            let entry_type = entry.header().entry_type();
            if entry_type.is_dir() {
                assert_eq!(entry.size(), 0, "directory entries carry no payload");
                roots_directory_seen |= path == "roots";
                cache_root_seen |= path == expected_root;
                continue;
            }
            assert!(
                entry_type.is_file(),
                "fixture has only files and directories"
            );
            if path == expected_member {
                assert!(member.is_none(), "provider member is unique");
                member = Some((entry.raw_file_position(), entry.size()));
            }
        }
        (roots_directory_seen, cache_root_seen, member)
    };

    assert!(roots_directory_seen, "archive has a roots directory");
    assert!(cache_root_seen, "archive has the provider cache root");
    member.expect("archive has the provider member")
}
