// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{ReservationId, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const PROBE: u64 = (172 << 30) + (1 << 30);
const NO_SPACE: &str = "FAKE_GUEST_CAP_ERROR=No space left on device\n";

#[test]
fn rust_runner_refuses_a_mount_before_it_repairs_anything() -> Check {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_GUEST_MOUNTS=/var/lib/velnor/tools btrfs\n")?;
    fixture.ensure_refused(RustRunnerFailure::MachineShape);
    assert_eq!(fixture.commands("install")?, 0);
    assert!(!fixture.guest("var/lib/velnor").exists());
    Ok(())
}

#[test]
fn rust_runner_never_writes_through_a_symlink_at_the_probe_or_the_record() -> Check {
    let mut followed = Vec::new();
    for planted in [".cap-probe", "cap-probe.new"] {
        let fixture = Fixture::ready()?;
        let victim = fixture.guest("etc/victim");
        std::fs::write(&victim, "keep")?;
        std::fs::create_dir_all(fixture.guest("var/lib/velnor"))?;
        std::os::unix::fs::symlink(&victim, fixture.guest(&format!("var/lib/velnor/{planted}")))?;
        fixture.launcher().ensure_machine()?;
        let record = fixture.guest("var/lib/velnor/cap-probe");
        let regular = !std::fs::symlink_metadata(&record)?.is_symlink();
        if std::fs::read_to_string(&victim)? != "keep" || !regular {
            followed.push(planted);
        }
    }
    assert!(followed.is_empty(), "followed: {followed:?}");
    Ok(())
}

#[test]
fn rust_runner_takes_no_space_left_as_proof_only_with_room_to_spare() -> Check {
    let refused = |free: u64| -> Check {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("{NO_SPACE}FAKE_GUEST_FREE={free}\n"))?;
        fixture.ensure_refused(RustRunnerFailure::MachineShape);
        assert!(
            !fixture.guest("var/lib/velnor/cap-probe").exists(),
            "{free}"
        );
        Ok(())
    };
    refused(0)?;
    refused(PROBE)?;
    let roomy = Fixture::ready()?;
    roomy.configure(&format!("{NO_SPACE}FAKE_GUEST_FREE={}\n", 2 * PROBE))?;
    roomy.launcher().ensure_machine()?;
    let quota = Fixture::ready()?;
    quota.configure("FAKE_GUEST_CAP_ERROR=Disk quota exceeded\nFAKE_GUEST_FREE=0\n")?;
    quota.launcher().ensure_machine()?;
    Ok(())
}

#[test]
fn rust_runner_sweep_never_crosses_a_mount_and_hands_the_reservation_to_quarantine() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    let precious = fixture.guest("srv/velnor/gen/work/12/tools/precious");
    let stale = fixture.guest("srv/velnor/gen/work/13/data");
    for file in [&precious, &stale] {
        std::fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        std::fs::write(file, "x")?;
    }
    std::fs::write(fixture.root.join("ps.table"), "")?;
    fixture.configure("FAKE_GUEST_MOUNTS=/srv/velnor/gen/work/12/tools btrfs\n")?;
    let left: Vec<u64> = fixture
        .launcher()
        .sweep()?
        .into_iter()
        .map(ReservationId::get)
        .collect();
    assert_eq!(left, [12]);
    assert!(precious.exists(), "the sweep went through a mount");
    assert!(!stale.exists());
    fixture.configure("")?;
    fixture.launcher().sweep()?;
    assert!(!precious.exists());
    Ok(())
}
