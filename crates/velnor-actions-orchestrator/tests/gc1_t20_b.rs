// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn index_members(plan: &SnapshotPlan) -> Result<Vec<CraftMember>, Box<dyn Error>> {
    let records = archived_projection_records(
        &plan
            .projection
            .iter()
            .map(|entry| entry.record.clone())
            .collect::<Vec<_>>(),
    )?;
    let file = |name: &str, bytes: Vec<u8>| CraftMember::File {
        name: name.to_owned(),
        bytes,
    };
    let mut members = vec![
        file("manifest.json", manifest_canonical_bytes(&plan.manifest)?),
        file("state-index.json", canonical_json_bytes(&plan.state_index)?),
        file("projection-index.json", canonical_json_bytes(&records)?),
        file(
            "required-output-index.json",
            canonical_json_bytes(&plan.required_outputs)?,
        ),
    ];
    for entry in &plan.state_index {
        let lane =
            lane_root_for(plan, &entry.path).ok_or_else(|| fail("state member lane missing"))?;
        let lane_relative = entry
            .path
            .split_once('/')
            .and_then(|(_, rest)| rest.split_once('/'))
            .map(|(_, rest)| rest)
            .ok_or_else(|| fail("state member shape"))?;
        let absolute = lane.join(lane_relative);
        match entry.kind.as_str() {
            "file" => members.push(file(&entry.path, fs::read(&absolute)?)),
            "directory" => members.push(CraftMember::Dir {
                name: entry.path.clone(),
            }),
            _ => members.push(CraftMember::Link {
                name: entry.path.clone(),
                target: entry
                    .link_target
                    .clone()
                    .ok_or_else(|| fail("link target missing"))?,
            }),
        }
    }
    Ok(members)
}

fn reject_bad_manifest(plan: &SnapshotPlan) -> Result<(), Box<dyn Error>> {
    must_fail!(
        parse_manifest_bytes(&tampered_manifest(plan, "schema")?),
        "manifest schema",
    );
    must_fail!(
        parse_manifest_bytes(&tampered_manifest(plan, "field")?),
        "manifest unknown field",
    );
    must_fail!(
        parse_manifest_bytes(br#"{"schema":1,"schema":1}"#),
        "manifest duplicate key",
    );
    let canonical = manifest_canonical_bytes(&plan.manifest)?;
    must_fail!(
        parse_manifest_bytes(&canonical[..canonical.len() / 2]),
        "manifest truncated",
    );
    let mut wrong_format = serde_json::to_value(&plan.manifest)?;
    wrong_format["archive_format"] = serde_json::Value::from("other-format");
    must_fail!(
        parse_manifest_bytes(&serde_json::to_vec(&wrong_format)?),
        "manifest archive format",
    );
    Ok(())
}

fn reject_bad_plan(fixture: &FullFixture) -> Result<(), Box<dyn Error>> {
    let mut inputs = fixture.inputs.clone();
    inputs.claimed_completed_recipe_ids = Vec::new();
    must_fail!(build_snapshot_plan(&inputs), "empty claimed recipes");
    let mut inputs = fixture.inputs.clone();
    inputs.claimed_completed_recipe_ids = vec!["clippy/lint".to_owned(), "build/test".to_owned()];
    must_fail!(build_snapshot_plan(&inputs), "unsorted claimed recipes");
    let mut inputs = fixture.inputs.clone();
    inputs
        .projection_declared
        .push(inputs.projection_declared[0].clone());
    must_fail!(
        build_snapshot_plan(&inputs),
        "duplicate declared projection"
    );
    let mut inputs = fixture.inputs.clone();
    inputs.required_output_paths = vec!["state/build/absent".to_owned()];
    must_fail!(build_snapshot_plan(&inputs), "missing required output");
    let mut inputs = fixture.inputs.clone();
    inputs.lanes.push(LaneRoot {
        lane_id: "ghost".to_owned(),
        root: fixture.temp.path().join("absent-lane"),
    });
    must_fail!(build_snapshot_plan(&inputs), "absent lane root");
    let mut inputs = fixture.inputs.clone();
    inputs.lanes.push(LaneRoot {
        lane_id: "copy".to_owned(),
        root: fixture.target.clone(),
    });
    must_fail!(build_snapshot_plan(&inputs), "overlapping lanes");
    let mut inputs = fixture.inputs.clone();
    inputs.lanes = vec![LaneRoot {
        lane_id: "build".to_owned(),
        root: PathBuf::from("relative-lane"),
    }];
    must_fail!(build_snapshot_plan(&inputs), "relative lane root");
    Ok(())
}

fn reject_bad_payload(fixture: &FullFixture, plan: &SnapshotPlan) -> Result<(), Box<dyn Error>> {
    write_file(&fixture.target.join("debug/app"), b"tampered", 0o755)?;
    let out_path = fixture.temp.path().join("tampered.tar.gz");
    must_fail!(
        pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
            plan,
            checkout_root: &fixture.checkout,
            staged_projection_root: &fixture.staged,
            out_path: &out_path,
        }),
        "tampered payload",
    );
    write_file(&fixture.target.join("debug/app"), &fixture.app_bytes, 0o755)?;
    set_mtime(
        &fixture.target.join("debug/app"),
        fixture.app_mtime.0,
        fixture.app_mtime.1,
    )?;
    Ok(())
}

fn reject_raw_hostile_archives(fixture: &FullFixture) -> Result<(), Box<dyn Error>> {
    let raw_hostile: Vec<(&str, Vec<Vec<u8>>)> = vec![
        ("escape", vec![raw_tar_member("../evil", b'0', "", b"evil")]),
        ("absolute", vec![raw_tar_member("/evil", b'0', "", b"evil")]),
        (
            "abs-link",
            vec![raw_tar_member("state/build/evil", b'2', "/etc/passwd", &[])],
        ),
        (
            "escape-link",
            vec![raw_tar_member("state/build/evil", b'2', "../../evil", &[])],
        ),
    ];
    for (case, members) in &raw_hostile {
        let archive = fixture.temp.path().join(format!("hostile-{case}.tar.gz"));
        raw_archive(&archive, members)?;
        let destination = fixture.temp.path().join(format!("hostile-{case}"));
        must_fail!(extract_snapshot_archive(&archive, &destination), case);
    }
    Ok(())
}

fn reject_crafted_hostile_archives(
    fixture: &FullFixture,
    plan: &SnapshotPlan,
    base: &[CraftMember],
) -> Result<(), Box<dyn Error>> {
    let hostile: Vec<(&str, Vec<CraftMember>)> = vec![
        (
            "duplicate",
            vec![
                CraftMember::File {
                    name: "manifest.json".to_owned(),
                    bytes: manifest_canonical_bytes(&plan.manifest)?,
                },
                CraftMember::File {
                    name: "manifest.json".to_owned(),
                    bytes: manifest_canonical_bytes(&plan.manifest)?,
                },
            ],
        ),
        (
            "git",
            vec![CraftMember::File {
                name: ".git/config".to_owned(),
                bytes: b"evil".to_vec(),
            }],
        ),
        (
            "hardlink",
            vec![CraftMember::Hard {
                name: "state/build/evil".to_owned(),
                target: "state/build/debug/app".to_owned(),
            }],
        ),
        (
            "prefix",
            vec![CraftMember::File {
                name: "other/payload".to_owned(),
                bytes: b"evil".to_vec(),
            }],
        ),
        (
            "fifo",
            vec![CraftMember::Fifo {
                name: "state/build/evil".to_owned(),
            }],
        ),
        ("inventory", base.iter().take(4).cloned().collect()),
    ];
    for (case, members) in &hostile {
        let archive = fixture.temp.path().join(format!("hostile-{case}.tar.gz"));
        craft_archive(&archive, members)?;
        let destination = fixture.temp.path().join(format!("hostile-{case}"));
        must_fail!(extract_snapshot_archive(&archive, &destination), case);
    }
    Ok(())
}

fn reject_digest_mismatch_archive(
    fixture: &FullFixture,
    plan: &SnapshotPlan,
    base: &[CraftMember],
) -> Result<(), Box<dyn Error>> {
    let mut digest_case = base.to_vec();
    let mut tweaked = serde_json::to_value(&plan.state_index)?;
    tweaked[0]["content_sha256"] = serde_json::Value::from("00".to_owned() + &"00".repeat(31));
    digest_case[1] = CraftMember::File {
        name: "state-index.json".to_owned(),
        bytes: serde_json::to_vec(&tweaked)?,
    };
    let digest_archive = fixture.temp.path().join("hostile-digest.tar.gz");
    craft_archive(&digest_archive, &digest_case)?;
    must_fail!(
        extract_snapshot_archive(&digest_archive, &fixture.temp.path().join("hostile-digest")),
        "digest",
    );
    Ok(())
}

fn reject_truncated_and_occupied(
    fixture: &FullFixture,
    plan: &SnapshotPlan,
) -> Result<(), Box<dyn Error>> {
    let packed = fixture.temp.path().join("truncated.tar.gz");
    pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
        plan,
        checkout_root: &fixture.checkout,
        staged_projection_root: &fixture.staged,
        out_path: &packed,
    })?;
    let bytes = fs::read(&packed)?;
    let cut = packed.with_extension("cut.gz");
    fs::write(&cut, &bytes[..bytes.len() / 2])?;
    must_fail!(
        extract_snapshot_archive(&cut, &fixture.temp.path().join("truncated")),
        "truncated gzip",
    );
    let taken = fixture.temp.path().join("restored-taken");
    fs::create_dir(&taken)?;
    must_fail!(
        extract_snapshot_archive(&packed, &taken),
        "existing destination"
    );
    Ok(())
}

fn reject_hostile_archives(
    fixture: &FullFixture,
    plan: &SnapshotPlan,
) -> Result<(), Box<dyn Error>> {
    let base = index_members(plan)?;
    reject_raw_hostile_archives(fixture)?;
    reject_crafted_hostile_archives(fixture, plan, &base)?;
    reject_digest_mismatch_archive(fixture, plan, &base)?;
    reject_truncated_and_occupied(fixture, plan)?;
    Ok(())
}

#[test]
fn gc1_t20_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let fixture = build_full_fixture()?;
    let plan = build_snapshot_plan(&fixture.inputs)?;
    reject_bad_manifest(&plan)?;
    reject_bad_plan(&fixture)?;
    let lane = tempfile::tempdir()?;
    write_file(&lane.path().join(".git/config"), b"git", 0o644)?;
    must_fail!(
        collect_state_index(
            &[LaneRoot {
                lane_id: "build".to_owned(),
                root: lane.path().to_owned(),
            }],
            &[],
        ),
        "git member",
    );
    let socket_lane = tempfile::tempdir()?;
    let _socket = UnixListener::bind(socket_lane.path().join("s.sock"))?;
    must_fail!(
        collect_state_index(
            &[LaneRoot {
                lane_id: "build".to_owned(),
                root: socket_lane.path().to_owned(),
            }],
            &[],
        ),
        "special file",
    );
    let mut duplicated = serde_json::to_value(&plan.state_index)?;
    let first = duplicated[0].clone();
    duplicated
        .as_array_mut()
        .ok_or_else(|| fail("state index not array"))?
        .push(first);
    must_fail!(
        parse_state_index_bytes(&serde_json::to_vec(&duplicated)?),
        "state index duplicate",
    );
    let mut bad_mtime = serde_json::to_value(&archived_projection_records(
        &plan
            .projection
            .iter()
            .map(|entry| entry.record.clone())
            .collect::<Vec<_>>(),
    )?)?;
    bad_mtime[0]["mtime"]["nanoseconds"] = serde_json::Value::from(2_000_000_000_u32);
    must_fail!(
        parse_projection_index_bytes(&serde_json::to_vec(&bad_mtime)?),
        "projection mtime",
    );
    reject_bad_payload(&fixture, &plan)?;
    reject_hostile_archives(&fixture, &plan)?;
    Ok(())
}
