// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{FlockOperation, flock};

use super::layout::root_of;
use super::{NET_LOCK, RESOLV_CONF};

const IP: &str = "/usr/sbin/ip";
const NFT: &str = "/usr/sbin/nft";
const NETNS_DIRECTORY: &str = "/run/netns";
const NETNS_PREFIX: &str = "velnor-job-";
const VETH_PREFIX: &str = "vj";
const TABLE: &str = "velnor_job";
const POOL_BASE: u32 = 0xc612_0000;
const SLOTS: u32 = 1 << 15;
const FORWARDING: &str = "/proc/sys/net/ipv4/ip_forward";
const SLOT_FILE: &str = "slot";
const REJECTED: [&str; 13] = [
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/3",
];

#[must_use]
pub(super) fn netns_name(reservation: u64) -> String {
    format!("{NETNS_PREFIX}{reservation}")
}

#[must_use]
pub(super) fn netns_path(reservation: u64) -> PathBuf {
    Path::new(NETNS_DIRECTORY).join(netns_name(reservation))
}

fn veth_name(slot: u32) -> String {
    format!("{VETH_PREFIX}{slot}")
}

pub(super) fn addresses(slot: u32) -> (Ipv4Addr, Ipv4Addr) {
    let base = POOL_BASE + slot * 2;
    (Ipv4Addr::from(base), Ipv4Addr::from(base + 1))
}

pub(super) fn resolver(resolv_conf: &str) -> Result<Ipv4Addr> {
    let mut found = resolv_conf.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("nameserver"))
            .then(|| fields.next())
            .flatten()
            .and_then(|address| address.parse::<Ipv4Addr>().ok())
    });
    match (found.next(), found.next()) {
        (Some(address), None) => Ok(address),
        _ => bail!("{RESOLV_CONF} does not name exactly one IPv4 nameserver"),
    }
}

pub(super) fn default_device(routes: &str) -> Result<String> {
    let mut devices = routes.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("default"))
            .then(|| fields.skip_while(|field| *field != "dev").nth(1))
            .flatten()
    });
    match (devices.next(), devices.next()) {
        (Some(device), None) => Ok(device.to_owned()),
        _ => bail!("the machine has no single IPv4 default route"),
    }
}

pub(super) fn ruleset(resolver: Ipv4Addr, device: &str) -> String {
    let rejected = REJECTED.join(", ");
    format!(
        "table inet {TABLE}\n\
delete table inet {TABLE}\n\
table inet {TABLE} {{\n\
  chain input {{\n\
    type filter hook input priority 0; policy accept;\n\
    iifname \"{VETH_PREFIX}*\" reject\n\
  }}\n\
  chain forward {{\n\
    type filter hook forward priority 0; policy accept;\n\
    oifname \"{VETH_PREFIX}*\" ct state established,related accept\n\
    oifname \"{VETH_PREFIX}*\" drop\n\
    iifname \"{VETH_PREFIX}*\" meta nfproto ipv6 drop\n\
    iifname \"{VETH_PREFIX}*\" ip daddr {resolver} udp dport 53 accept\n\
    iifname \"{VETH_PREFIX}*\" ip daddr {resolver} tcp dport 53 accept\n\
    iifname \"{VETH_PREFIX}*\" udp dport 53 drop\n\
    iifname \"{VETH_PREFIX}*\" tcp dport 53 drop\n\
    iifname \"{VETH_PREFIX}*\" ip daddr {{ {rejected} }} reject\n\
    iifname \"{VETH_PREFIX}*\" oifname != \"{device}\" drop\n\
    iifname \"{VETH_PREFIX}*\" accept\n\
  }}\n\
  chain postrouting {{\n\
    type nat hook postrouting priority 100; policy accept;\n\
    iifname \"{VETH_PREFIX}*\" oifname \"{device}\" masquerade\n\
  }}\n\
}}\n"
    )
}

fn tool(program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("run {program}"))?;
    ensure!(
        output.status.success(),
        "{program} {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn load_ruleset() -> Result<()> {
    let resolv =
        std::fs::read_to_string(RESOLV_CONF).with_context(|| format!("read {RESOLV_CONF}"))?;
    let device = default_device(&tool(IP, &["-4", "route", "show", "default"])?)?;
    let text = ruleset(resolver(&resolv)?, &device);
    let mut child = Command::new(NFT)
        .args(["-f", "-"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("run nft")?;
    child
        .stdin
        .take()
        .context("open the nft input")?
        .write_all(text.as_bytes())
        .context("write the nft ruleset")?;
    let finished = child.wait_with_output().context("wait for nft")?;
    ensure!(
        finished.status.success(),
        "nft refused the job ruleset: {}",
        String::from_utf8_lossy(&finished.stderr).trim()
    );
    std::fs::write(FORWARDING, "1").context("enable IPv4 forwarding")
}

fn in_use(slot: u32) -> bool {
    Path::new("/sys/class/net").join(veth_name(slot)).exists()
}

fn take_slot(reservation: u64) -> Result<u32> {
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(NET_LOCK)
        .with_context(|| format!("open {NET_LOCK}; provision the machine first"))?;
    flock(&lock, FlockOperation::LockExclusive).context("lock the network slots")?;
    let slot = (0..SLOTS)
        .find(|slot| !in_use(*slot))
        .context("all network slots are taken")?;
    let name = veth_name(slot);
    let namespace = netns_name(reservation);
    let (host, _) = addresses(slot);
    tool(
        IP,
        &[
            "link", "add", &name, "type", "veth", "peer", "name", "eth0", "netns", &namespace,
        ],
    )?;
    tool(IP, &["addr", "add", &format!("{host}/31"), "dev", &name])?;
    tool(IP, &["link", "set", &name, "up"])?;
    Ok(slot)
}

pub(super) fn up(reservation: u64) -> Result<()> {
    down(reservation)?;
    load_ruleset()?;
    let namespace = netns_name(reservation);
    tool(IP, &["netns", "add", &namespace])?;
    let slot = take_slot(reservation)?;
    std::fs::write(root_of(reservation).join(SLOT_FILE), slot.to_string())
        .context("record the network slot")?;
    let (host, peer) = addresses(slot);
    tool(
        IP,
        &[
            "-n",
            &namespace,
            "addr",
            "add",
            &format!("{peer}/31"),
            "dev",
            "eth0",
        ],
    )?;
    for device in ["eth0", "lo"] {
        tool(IP, &["-n", &namespace, "link", "set", device, "up"])?;
    }
    tool(
        IP,
        &[
            "-n",
            &namespace,
            "route",
            "add",
            "default",
            "via",
            &host.to_string(),
        ],
    )?;
    Ok(())
}

fn ignoring_absence(result: Result<String>) -> Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(error)
            if error.to_string().contains("Cannot find device")
                || error.to_string().contains("No such file")
                || error.to_string().contains("Cannot open network namespace") =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(super) fn down(reservation: u64) -> Result<()> {
    let recorded = std::fs::read_to_string(root_of(reservation).join(SLOT_FILE));
    if let Some(slot) = recorded
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
    {
        ignoring_absence(tool(IP, &["link", "del", &veth_name(slot)]))?;
    }
    if netns_path(reservation).exists() {
        ignoring_absence(tool(IP, &["netns", "del", &netns_name(reservation)]))?;
    }
    Ok(())
}

pub(super) fn down_all() -> Result<()> {
    let links = match std::fs::read_dir("/sys/class/net") {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("list the network devices"),
    };
    for entry in links.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(VETH_PREFIX)
            && name[VETH_PREFIX.len()..]
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        {
            ignoring_absence(tool(IP, &["link", "del", &name]))?;
        }
    }
    if let Ok(entries) = std::fs::read_dir(NETNS_DIRECTORY) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(NETNS_PREFIX) {
                ignoring_absence(tool(IP, &["netns", "del", &name]))?;
            }
        }
    }
    Ok(())
}
