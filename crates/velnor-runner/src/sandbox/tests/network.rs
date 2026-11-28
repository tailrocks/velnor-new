// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::Ipv4Addr;

use super::super::net::{addresses, default_device, resolver, ruleset};

#[test]
fn the_resolver_is_the_one_ipv4_nameserver_or_the_job_gets_no_network() {
    let one = "search orb.local\nnameserver 198.19.248.200\nnameserver fd07:b51a:cc66::1\n";
    assert_eq!(resolver(one).ok(), Some(Ipv4Addr::new(198, 19, 248, 200)));
    assert!(resolver("").is_err());
    assert!(resolver("nameserver fd07::1\n").is_err());
    assert!(resolver("nameserver 10.0.0.1\nnameserver 10.0.0.2\n").is_err());
}

#[test]
fn the_default_route_device_is_the_only_one_or_the_job_gets_no_network() {
    assert_eq!(
        default_device(
            "default via 192.168.139.1 dev eth0 proto dhcp src 192.168.139.91 metric 100\n"
        )
        .ok(),
        Some("eth0".to_owned())
    );
    assert!(default_device("").is_err());
    assert!(
        default_device("default via 10.0.0.1 dev eth0\ndefault via 10.0.1.1 dev eth1\n").is_err()
    );
}

#[test]
fn the_job_ruleset_orders_dns_to_the_resolver_before_the_rejected_ranges() {
    let text = ruleset(Ipv4Addr::new(198, 19, 248, 200), "eth0");
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic_free(needle));
    assert!(at("delete table inet velnor_job") < at("chain input"));
    assert!(text.contains("iifname \"vj*\" reject"));
    assert!(text.contains("iifname \"vj*\" meta nfproto ipv6 drop"));
    assert!(text.contains("oifname \"vj*\" drop"));
    assert!(text.contains("oifname != \"eth0\" drop"));
    assert!(text.contains("masquerade"));
    assert!(at("ip daddr 198.19.248.200 udp dport 53 accept") < at("198.18.0.0/15"));
    assert!(at("udp dport 53 drop") < at("198.18.0.0/15"));
    for range in [
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
    ] {
        assert!(text.contains(range), "{range}");
    }
}

fn panic_free(needle: &str) -> usize {
    assert!(needle.is_empty(), "the ruleset has no {needle}");
    0
}

#[test]
fn job_addresses_are_distinct_pairs_inside_the_198_18_16_pool() {
    assert_eq!(
        addresses(0),
        (Ipv4Addr::new(198, 18, 0, 0), Ipv4Addr::new(198, 18, 0, 1))
    );
    assert_eq!(
        addresses(1),
        (Ipv4Addr::new(198, 18, 0, 2), Ipv4Addr::new(198, 18, 0, 3))
    );
    assert_eq!(
        addresses(32767),
        (
            Ipv4Addr::new(198, 18, 255, 254),
            Ipv4Addr::new(198, 18, 255, 255)
        )
    );
}

const MOUNTINFO: &str = "24 1 0:21 /scon/containers/X / rw,noatime - btrfs /dev/vdb1 rw,ssd,subvolid=291,subvol=/scon/containers/X\n30 24 0:30 / /run rw - tmpfs tmpfs rw\n41 24 0:41 /sub /srv/velnor/gen rw - btrfs /dev/vdc1 rw,subvolid=300,subvol=/sub\n";

#[test]
fn the_floor_finds_the_btrfs_mount_and_subvolume_of_the_generations() {
    use super::super::floor::{Mount, mount_of};
    use std::path::Path;
    assert_eq!(
        mount_of(MOUNTINFO, Path::new("/srv/velnor/gen")),
        Some(Mount {
            device: "/dev/vdc1".to_owned(),
            subvolume: 300
        })
    );
    assert_eq!(
        mount_of(MOUNTINFO, Path::new("/srv/other")),
        Some(Mount {
            device: "/dev/vdb1".to_owned(),
            subvolume: 291
        })
    );
    assert_eq!(mount_of("", Path::new("/srv")), None);
}

#[test]
fn the_floor_counts_the_live_reservation_and_fails_closed_on_anything_unreadable() {
    use super::super::floor::free_bytes_of;
    assert_eq!(
        free_bytes_of("4802846720\n", "1073741824\n", "8589934592\n"),
        Some(2_713_346_048)
    );
    assert_eq!(free_bytes_of("9", "9", "5"), Some(0));
    assert_eq!(free_bytes_of("1", "0", "0"), None);
    assert_eq!(free_bytes_of("9", "0", "none"), None);
    assert_eq!(free_bytes_of("9", "none", "20"), None);
    assert_eq!(free_bytes_of("none", "0", "20"), None);
}
