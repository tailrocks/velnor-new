use super::parser::{backing_mount, parse_overlay_paths, parse_root_mount};
use super::{strip_root_prefix, validate_docker_root_path};

#[test]
fn overlay_paths_require_a_read_write_layer_and_work_directory() {
    let paths = parse_overlay_paths(
        "rw,lowerdir=/var/lib/docker/lower,upperdir=/var/lib/docker/upper,workdir=/var/lib/docker/work,index=off",
    );
    assert!(paths.is_ok());
    assert!(parse_overlay_paths("ro,lowerdir=/var/lib/docker/lower").is_err());
    assert!(
        parse_overlay_paths("ro,lowerdir=/var/lib/docker/lower,upperdir=/var/lib/docker/upper")
            .is_err()
    );
}

#[test]
fn overlay_parser_rejects_duplicate_and_unsupported_options() {
    assert!(
        parse_overlay_paths("ro,lowerdir=/d/l,upperdir=/d/u,upperdir=/d/u2,workdir=/d/w").is_err()
    );
    assert!(parse_overlay_paths("ro,lowerdir=/d/l,upperdir=/d/u,workdir=/d/w,unknown=on").is_err());
}

#[test]
fn overlay_parser_rejects_bad_lower_directories() {
    assert!(parse_overlay_paths("ro,lowerdir=/d/l:/d/l,upperdir=/d/u,workdir=/d/w").is_err());
    assert!(parse_overlay_paths("ro,lowerdir=/d/../l,upperdir=/d/u,workdir=/d/w").is_err());
    assert!(parse_overlay_paths("ro,lowerdir=,upperdir=/d/u,workdir=/d/w").is_err());
}

#[test]
fn docker_root_paths_must_be_canonical_absolute_paths() {
    assert!(validate_docker_root_path("/var/lib/docker").is_ok());
    for path in ["", "relative", "/var/../docker", "/var//docker", "/var/"] {
        assert!(validate_docker_root_path(path).is_err());
    }
}

#[test]
fn backing_paths_require_a_component_boundary_under_the_docker_root() {
    assert!(
        strip_root_prefix("/var/lib/docker/overlay2/abc/diff", "/var/lib/docker")
            .is_ok_and(|path| path == "overlay2/abc/diff")
    );
    assert!(strip_root_prefix("/var/lib/docker-copy/overlay2/x", "/var/lib/docker").is_err());
    assert!(strip_root_prefix("/var/lib/docker/../foreign", "/var/lib/docker").is_err());
}

#[test]
fn root_mount_parser_requires_one_valid_root_row() {
    let row = b"36 25 0:32 / / ro,relatime - overlay overlay rw,lowerdir=/d/l,upperdir=/d/u,workdir=/d/w\n";
    let mount = parse_root_mount(row);
    assert!(mount.as_ref().is_ok_and(|value| {
        value.mount_id == 36
            && value.read_only
            && value.filesystem_type == "overlay"
            && value.mount_source == "overlay"
    }));
    assert!(
        parse_root_mount(b"36 25 0:32 / / rw - overlay overlay rw\n")
            .is_ok_and(|value| !value.read_only)
    );
    assert!(parse_root_mount(&[row.as_slice(), row.as_slice()].concat()).is_err());
}

#[test]
fn backing_mount_uses_the_nearest_mountpoint() {
    let input = b"1 0 8:1 / / rw - ext4 /dev/root rw\n36 1 8:2 / /velnor/docker-root ro - ext4 /dev/vdb rw\n37 36 8:3 /foreign /velnor/docker-root/overlay2 ro - ext4 /dev/foreign rw\n";
    let selected = backing_mount(input, "/velnor/docker-root/overlay2/abc/diff");
    assert!(selected.as_ref().is_ok_and(|mount| {
        mount.mount_id == 37
            && mount.source == "/dev/foreign"
            && mount.root == "/foreign"
            && mount.read_only
    }));
    let root = backing_mount(input, "/velnor/docker-root/containers/abc");
    assert!(root.as_ref().is_ok_and(|mount| {
        mount.mount_id == 36 && mount.source == "/dev/vdb" && mount.read_only
    }));
}

#[test]
fn backing_mount_requires_one_unambiguous_nearest_row() {
    let input = b"1 0 8:1 / / rw - ext4 /dev/root rw\n36 1 8:2 / /velnor/docker-root ro - ext4 /dev/a rw\n37 1 8:3 / /velnor/docker-root ro - ext4 /dev/b rw\n";
    assert!(backing_mount(input, "/velnor/docker-root/overlay2/abc").is_err());
    assert!(
        backing_mount(input, "/velnor/docker-root-copy/abc").is_ok_and(|mount| mount.mount_id == 1)
    );
}
