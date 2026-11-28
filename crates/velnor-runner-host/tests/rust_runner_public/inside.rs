// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::RustRunnerFailure;

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;
type Row = (&'static str, fn(&Fixture) -> Check);

const CAP_BYTES: u64 = 172 << 30;

fn plant(fixture: &Fixture, setting: &str) -> Check {
    fixture.configure(&format!("{setting}\n"))
}

#[test]
fn rust_runner_installs_nftables_and_btrfs_progs_once_and_needs_their_tools() -> Check {
    let fixture = Fixture::ready()?;
    assert!(!fixture.guest("usr/sbin/nft").exists());
    fixture.launcher().ensure_machine()?;
    fixture.launcher().ensure_machine()?;
    assert!(fixture.guest("usr/sbin/nft").exists());
    assert!(fixture.guest("usr/sbin/btrfs").exists());
    assert_eq!(fixture.commands("apt-get")?, 1);
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_without_nft_ip_or_btrfs() -> Check {
    let nft = Fixture::ready()?;
    plant(&nft, "FAKE_GUEST_NO_NFT_PACKAGE=1")?;
    nft.ensure_refused(RustRunnerFailure::MachineTools);
    let ip = Fixture::ready()?;
    std::fs::remove_file(ip.guest("usr/sbin/ip"))?;
    ip.ensure_refused(RustRunnerFailure::MachineTools);
    let btrfs = Fixture::ready()?;
    plant(&btrfs, "FAKE_GUEST_NO_BTRFS_PACKAGE=1")?;
    btrfs.ensure_refused(RustRunnerFailure::MachineTools);
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_that_still_reaches_the_mac() -> Check {
    let rows: [Row; 14] = [
        ("a /mnt/mac directory", |f| {
            std::fs::create_dir(f.guest("mnt/mac"))?;
            Ok(())
        }),
        ("a dangling /mnt/mac link", |f| {
            std::os::unix::fs::symlink("/nowhere", f.guest("mnt/mac"))?;
            Ok(())
        }),
        ("a socket under /opt/orbstack-guest", |f| {
            plant(f, "FAKE_GUEST_SOCKETS=/opt/orbstack-guest/run/hcontrol")
        }),
        ("mac uname that works", |f| {
            plant(f, "FAKE_GUEST_MAC_WORKS=1")
        }),
        ("a second virtiofs mount", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/mnt/home virtiofs")
        }),
        ("a 9p mount", |f| plant(f, "FAKE_GUEST_MOUNTS=/mnt/x 9p")),
        ("a fuse mount", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/mnt/x fuse")
        }),
        ("a fuse subtype mount", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/mnt/x fuse.sshfs")
        }),
        ("the former host share still mounted", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/var/lib/velnor-host virtiofs")
        }),
        (
            "a symlinked /opt/orbstack-guest with a socket behind it",
            |f| {
                std::fs::remove_dir_all(f.guest("opt/orbstack-guest"))?;
                std::os::unix::fs::symlink("/nowhere", f.guest("opt/orbstack-guest"))?;
                plant(f, "FAKE_GUEST_SOCKETS=/opt/orbstack-guest/run/hcontrol")
            },
        ),
        ("an orb failure instead of the mac refusal", |f| {
            plant(f, "FAKE_GUEST_MAC_TRANSPORT=1")
        }),
        ("a mac probe that prints no marker", |f| {
            plant(f, "FAKE_GUEST_MAC_SILENT=1")
        }),
        ("a qgroup limit above the cap", |f| {
            plant(f, &format!("FAKE_GUEST_QGROUP={}", CAP_BYTES + 1))
        }),
        ("no qgroup limit", |f| plant(f, "FAKE_GUEST_QGROUP=0")),
    ];
    assert_shape_refused(&rows)
}

fn assert_shape_refused(rows: &[Row]) -> Check {
    let mut accepted = Vec::new();
    for (name, mutate) in rows {
        let fixture = Fixture::ready()?;
        mutate(&fixture)?;
        let refused = fixture.launcher().ensure_machine().err()
            == Some(velnor_runner_host::HostError::RustRunner(
                RustRunnerFailure::MachineShape,
            ));
        if !refused || fixture.guest("var/lib/velnor/bin/velnor-runner").exists() {
            accepted.push(*name);
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    Ok(())
}

#[test]
fn rust_runner_refuses_a_layout_that_is_mounted_over_or_split_from_the_capped_subvolume() -> Check {
    let rows: [Row; 7] = [
        ("a btrfs bind mount in the store", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/var/lib/velnor/tools btrfs")
        }),
        ("a btrfs bind mount over gen", |f| {
            plant(f, "FAKE_GUEST_MOUNTS=/srv/velnor/gen btrfs")
        }),
        ("a nested subvolume at the store", |f| {
            plant(f, "FAKE_GUEST_NESTED=/var/lib/velnor")
        }),
        ("a nested subvolume at /srv/velnor", |f| {
            plant(f, "FAKE_GUEST_NESTED=/srv/velnor")
        }),
        ("a nested subvolume at gen", |f| {
            plant(f, "FAKE_GUEST_NESTED=/srv/velnor/gen")
        }),
        ("a nested subvolume at gen/work", |f| {
            plant(f, "FAKE_GUEST_NESTED=/srv/velnor/gen/work")
        }),
        ("a nested subvolume at gen/stage", |f| {
            plant(f, "FAKE_GUEST_NESTED=/srv/velnor/gen/stage")
        }),
    ];
    assert_shape_refused(&rows)
}

#[test]
fn rust_runner_accepts_the_mounts_and_quota_of_a_confined_machine() -> Check {
    let fixture = Fixture::ready()?;
    plant(
        &fixture,
        "FAKE_GUEST_MOUNTS=/sys/fs/fuse/connections fusectl;/dev/fuse devtmpfs;/srv/velnor/gen/work/12/tools btrfs;/srv/velnor/gen/stage/12 btrfs",
    )?;
    fixture.launcher().ensure_machine()?;
    Ok(())
}
