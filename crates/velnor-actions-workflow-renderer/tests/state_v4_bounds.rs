// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;
use std::process::{Command, Stdio};

use velnor_actions_workflow_renderer::steps::StateCeilings;

use crate::gc1_mbx_stub::{EXPORT_FILES_ENV, EXPORT_SPARSE_BYTES_ENV, ZSTD_PAD_ENV};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_scene_bounded::scripts_with;
use crate::gc1_ustar::{Archive, decode_zstd, list_members, pad_to};

const KIB: u64 = 1024;
const STORED: u64 = 4 * KIB;
const EXTRACTED: u64 = 1024 * KIB;
const MEMBERS: u64 = 1_000;
const SHIPPED_STORED: u64 = 1_610_612_736;
const SHIPPED_EXTRACTED: u64 = 12_884_901_888;
const SHIPPED_MEMBERS: u64 = 100_000;

fn bounded(name: &str, stored: u64, extracted: u64, members: u64) -> Result<Scene, Box<dyn Error>> {
    Scene::new(
        name,
        scripts_with(StateCeilings::new(stored, extracted, members))?,
    )
}

fn payload_dir(scene: &Scene) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    Ok(())
}

fn valid_payload(scene: &Scene) -> Result<(), Box<dyn Error>> {
    payload_dir(scene)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.finish()?;
    Ok(())
}

fn du_bytes(path: &Path) -> Result<u64, Box<dyn Error>> {
    let output = Command::new("du").arg("-sb").arg(path).output()?;
    if !output.status.success() {
        return Err(fail("du -sb failed"));
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or_else(|| fail("du printed nothing"))?
        .parse()
        .map_err(Into::into)
}

fn extracted_bytes(payload: &Path) -> Result<u64, Box<dyn Error>> {
    let oracle = tempfile::tempdir()?;
    let plain = oracle.path().join("oracle.tar");
    decode_zstd(payload, &plain)?;
    let tree = oracle.path().join("tree");
    std::fs::create_dir(&tree)?;
    let status = Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(&tree)
        .stdin(std::fs::File::open(&plain)?)
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(fail("oracle extraction failed"));
    }
    du_bytes(&tree.join("state"))
}

fn export_bytes(scene: &Scene, env: &[(&str, &str)]) -> Result<u64, Box<dyn Error>> {
    let oracle = tempfile::tempdir()?;
    scene.stub_export(&oracle.path().join("state"), env)?;
    du_bytes(&oracle.path().join("state"))
}

fn member_count(scene: &Scene) -> Result<u64, Box<dyn Error>> {
    scene.pack_export("fresh")?;
    Ok(u64::try_from(
        list_members(&scene.payload())?.lines().count(),
    )?)
}

fn require_imported(scene: &Scene, label: &str) -> Result<(), Box<dyn Error>> {
    let record = std::fs::read_to_string(scene.recovery_record())
        .map_err(|error| fail(&format!("{label}: no recovery record ({error})")))?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!(
            "{label}: recover lost its exact hit: {record}"
        )));
    }
    if std::fs::read_to_string(scene.store().join("cas/object"))? != "old" {
        return Err(fail(&format!("{label}: store lost the restored object")));
    }
    scene.assert_clean_exit()
}

fn require_in(output: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if output.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label}: lost {needle}: {output}")))
    }
}

fn require_not_published(
    scene: &Scene,
    output: &str,
    reason: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    require_in(output, &format!("not published ({reason})"), label)?;
    if scene.output_value("exported")? != "false" || scene.output_value("key").is_ok() {
        return Err(fail(&format!("{label}: a refusal must publish no key")));
    }
    let runner = &scene.runner;
    for path in [
        scene.payload(),
        runner.join("velnor/state-publication.staged"),
        runner.join("velnor/state-publication.record"),
    ] {
        if !Scene::gone(&path) {
            return Err(fail(&format!("{label}: left {}", path.display())));
        }
    }
    Ok(())
}

fn producer_scene(
    name: &str,
    stored: u64,
    extracted: u64,
    members: u64,
) -> Result<Scene, Box<dyn Error>> {
    let scene = bounded(name, stored, extracted, members)?;
    scene.discover()?;
    scene.mark_producer()?;
    Ok(scene)
}

#[test]
fn recover_over_stored_boundary() -> Result<(), Box<dyn Error>> {
    let scene = bounded("v4-stored-exact", STORED, EXTRACTED, MEMBERS)?;
    let key = scene.discover()?;
    valid_payload(&scene)?;
    pad_to(&scene.payload(), STORED)?;
    scene.recover(&key.exact(), &[], None)?;
    require_imported(&scene, "payload of exactly the stored bound")?;
    scene.finish()?;

    let scene = bounded("v4-stored-over", STORED, EXTRACTED, MEMBERS)?;
    let key = scene.discover()?;
    valid_payload(&scene)?;
    pad_to(&scene.payload(), STORED + 1)?;
    let real = std::fs::metadata(scene.payload())?.len();
    scene.shim_failure("zstd")?;
    let output = scene.recover(&key.exact(), &[], None)?;
    scene.assert_cold(&output, "over_stored")?;
    require_in(
        &output,
        &format!("over_stored stored={real} ceiling={STORED}"),
        "payload one byte over the stored bound",
    )?;
    if real != STORED + 1 || output.contains("payload unreadable") {
        return Err(fail(&format!(
            "the size check ran after a decode: {output}"
        )));
    }
    scene.finish()?;

    let scene = bounded("v4-stored-zeros", STORED, EXTRACTED, MEMBERS)?;
    let key = scene.discover()?;
    payload_dir(&scene)?;
    std::fs::File::create(scene.payload())?.set_len(STORED)?;
    let output = scene.recover(&key.exact(), &[], None)?;
    scene.assert_cold(&output, "refused-format")?;
    if output.contains("over_stored") {
        return Err(fail(&format!("a payload at the bound was over: {output}")));
    }
    scene.finish()
}

#[test]
fn recover_over_extracted_boundary() -> Result<(), Box<dyn Error>> {
    let oracle = Scene::new("v4-extracted-oracle", crate::gc1_t53::scripts()?)?;
    valid_payload(&oracle)?;
    let extracted = extracted_bytes(&oracle.payload())?;
    oracle.finish()?;

    let scene = bounded("v4-extracted-exact", EXTRACTED, extracted, MEMBERS)?;
    let key = scene.discover()?;
    valid_payload(&scene)?;
    scene.recover(&key.exact(), &[], None)?;
    require_imported(&scene, "tree of exactly the extracted bound")?;
    scene.finish()?;

    let scene = bounded("v4-extracted-over", EXTRACTED, extracted - 1, MEMBERS)?;
    let key = scene.discover()?;
    valid_payload(&scene)?;
    let output = scene.recover(&key.exact(), &[], None)?;
    scene.assert_cold(&output, "over_extracted")?;
    require_in(
        &output,
        &format!(
            "over_extracted extracted={extracted} ceiling={}",
            extracted - 1
        ),
        "tree one byte over the extracted bound",
    )?;
    scene.finish()
}

#[test]
fn recover_stream_bomb() -> Result<(), Box<dyn Error>> {
    let members = 100;
    let ceilings = StateCeilings::new(EXTRACTED, EXTRACTED, members);
    let cap = ceilings.stream_cap();
    let scene = Scene::new("v4-bomb", scripts_with(ceilings)?)?;
    let key = scene.discover()?;
    payload_dir(&scene)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.zeros("state/bomb", 4 * cap)?;
    archive.finish()?;
    let stored = std::fs::metadata(scene.payload())?.len();
    if stored >= EXTRACTED {
        return Err(fail("the bomb does not compress below the stored bound"));
    }
    let limit = (cap + 64 * KIB) / 512;
    let output = scene.recover(&key.exact(), &[], Some(limit))?;
    scene.assert_cold(&output, "over_extracted")?;
    require_in(
        &output,
        &format!("over_extracted extracted>={} ceiling={EXTRACTED}", cap + 1),
        "stream bomb",
    )?;
    for file in crate::gc1_scene::files_under(&scene.runner)? {
        if std::fs::symlink_metadata(&file)?.len() > cap {
            return Err(fail(&format!("inflate residue: {}", file.display())));
        }
    }
    scene.finish()
}

#[test]
fn shipped_ceilings_refuse_at_documented_bytes() -> Result<(), Box<dyn Error>> {
    let shipped = StateCeilings::V3;
    if (shipped.stored(), shipped.extracted(), shipped.members())
        != (SHIPPED_STORED, SHIPPED_EXTRACTED, SHIPPED_MEMBERS)
    {
        return Err(fail("the shipped ceilings drifted"));
    }
    for (name, len, token) in [
        ("v4-shipped-over", SHIPPED_STORED + 1, "over_stored"),
        ("v4-shipped-exact", SHIPPED_STORED, "refused-format"),
    ] {
        let scene = Scene::new(name, crate::gc1_t53::scripts()?)?;
        let key = scene.discover()?;
        payload_dir(&scene)?;
        std::fs::File::create(scene.payload())?.set_len(len)?;
        let output = scene.recover(&key.exact(), &[], None)?;
        scene.assert_cold(&output, token)?;
        let evidence = format!("over_stored stored={len} ceiling={SHIPPED_STORED}");
        if output.contains(&evidence) != (len > SHIPPED_STORED) {
            return Err(fail(&format!("{name}: stored evidence is wrong: {output}")));
        }
        scene.finish()?;
    }
    let scene = Scene::new("v4-shipped-publish", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    let base = export_bytes(&scene, &[])?;
    let sparse = (SHIPPED_EXTRACTED + 1 - base).to_string();
    let env = [(EXPORT_SPARSE_BYTES_ENV, sparse.as_str())];
    if export_bytes(&scene, &env)? != SHIPPED_EXTRACTED + 1 {
        return Err(fail("the sparse export is not one byte over the bound"));
    }
    let output = scene.export(&env)?;
    require_in(
        &output,
        &format!(
            "over_extracted extracted={} ceiling={SHIPPED_EXTRACTED}",
            SHIPPED_EXTRACTED + 1
        ),
        "publish at the shipped extracted bound",
    )?;
    require_not_published(&scene, &output, "over_extracted", "shipped publish")?;
    scene.finish()
}

#[test]
fn publish_over_stored_boundary() -> Result<(), Box<dyn Error>> {
    let scene = producer_scene("v4-pub-stored-exact", STORED, EXTRACTED, MEMBERS)?;
    scene.install_zstd_pad()?;
    scene.export(&[(ZSTD_PAD_ENV, &STORED.to_string())])?;
    if scene.output_value("exported")? != "true"
        || std::fs::metadata(scene.payload())?.len() != STORED
    {
        return Err(fail("a payload of exactly the stored bound was refused"));
    }
    scene.finish()?;

    let scene = producer_scene("v4-pub-stored-over", STORED, EXTRACTED, MEMBERS)?;
    scene.install_zstd_pad()?;
    let output = scene.export(&[(ZSTD_PAD_ENV, &(STORED + 1).to_string())])?;
    require_in(
        &output,
        &format!("over_stored stored={} ceiling={STORED}", STORED + 1),
        "publish one byte over the stored bound",
    )?;
    require_not_published(&scene, &output, "over_stored", "stored refusal")?;
    scene.finish()
}

#[test]
fn publish_over_extracted_boundary() -> Result<(), Box<dyn Error>> {
    let probe = bounded("v4-pub-extracted-probe", STORED, EXTRACTED, MEMBERS)?;
    let extracted = export_bytes(&probe, &[])?;
    probe.finish()?;

    let scene = producer_scene("v4-pub-extracted-exact", EXTRACTED, extracted, MEMBERS)?;
    scene.export(&[])?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("an export of exactly the extracted bound was refused"));
    }
    scene.finish()?;

    let scene = producer_scene("v4-pub-extracted-over", EXTRACTED, extracted - 1, MEMBERS)?;
    let output = scene.export(&[])?;
    require_in(
        &output,
        &format!(
            "over_extracted extracted={extracted} ceiling={}",
            extracted - 1
        ),
        "publish one byte over the extracted bound",
    )?;
    require_not_published(&scene, &output, "over_extracted", "extracted refusal")?;
    scene.finish()
}

#[test]
fn publish_member_bound() -> Result<(), Box<dyn Error>> {
    let probe = bounded("v4-pub-members-probe", STORED, EXTRACTED, MEMBERS)?;
    let base = member_count(&probe)?;
    probe.finish()?;

    let scene = producer_scene("v4-pub-members-exact", EXTRACTED, EXTRACTED, base)?;
    scene.export(&[])?;
    if scene.output_value("exported")? != "true" {
        return Err(fail("an export of exactly the member bound was refused"));
    }
    scene.finish()?;

    let scene = producer_scene("v4-pub-members-over", EXTRACTED, EXTRACTED, base - 1)?;
    let output = scene.export(&[])?;
    require_not_published(&scene, &output, "member_over_budget", "member refusal")?;
    scene.finish()?;

    let scene = Scene::new("v4-pub-members-shipped", crate::gc1_t53::scripts()?)?;
    scene.discover()?;
    scene.mark_producer()?;
    let files = (SHIPPED_MEMBERS + 1 - base).to_string();
    let output = scene.export(&[(EXPORT_FILES_ENV, files.as_str())])?;
    require_not_published(
        &scene,
        &output,
        "member_over_budget",
        "shipped member refusal",
    )?;
    scene.finish()
}
