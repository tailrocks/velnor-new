// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) fn environment(cap_gib: u64) -> String {
    format!("FAKE_GUEST_CAP_BYTES={}\n", cap_gib << 30)
}

pub(crate) fn machines(
    store: &Path,
    cap_gib: u64,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> String {
    let mut machine = serde_json::json!({
        "name": "velnor",
        "image": {"distro": "ubuntu", "arch": "arm64"},
        "config": {
            "isolated": true,
            "isolate_network": true,
            "forward_ssh_agent": false,
            "disk_limit_bytes": cap_gib << 30,
            "mounts": [{"source": store, "destination": "/var/lib/velnor-host"}],
        },
    });
    mutate(&mut machine["config"]);
    serde_json::Value::Array(vec![machine]).to_string()
}

pub(crate) fn install_guest_module(root: &Path) -> io::Result<()> {
    for name in [
        "fake_orb_guest.ts",
        "fake_orb_base.ts",
        "fake_orb_system.ts",
    ] {
        let module = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::copy(module, root.join(name))?;
    }
    Ok(())
}

pub(crate) fn prepare_guest(root: &Path) -> io::Result<()> {
    for path in [
        "guest/var/lib",
        "guest/srv",
        "guest/usr/sbin",
        "guest/etc/sudoers.d",
        "guest/mnt",
        "guest/opt/orbstack-guest/run",
    ] {
        std::fs::create_dir_all(root.join(path))?;
    }
    std::fs::write(root.join("guest/usr/sbin/ip"), "")?;
    std::fs::write(
        root.join("guest/etc/passwd"),
        "root:x:0:0:root:/root:/bin/bash\n",
    )?;
    for path in ["guest", "guest/var", "guest/var/lib", "guest/srv"] {
        std::fs::set_permissions(root.join(path), std::fs::Permissions::from_mode(0o755))?;
    }
    let runner = root.join("host-runner");
    std::fs::write(&runner, "")?;
    std::fs::set_permissions(runner, std::fs::Permissions::from_mode(0o755))
}
