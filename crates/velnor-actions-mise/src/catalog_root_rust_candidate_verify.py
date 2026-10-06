"""Verify the fixed RootLinux Rust candidate mirror before native installation."""


_V2_MANIFEST = "channel-rust-1.98.1.toml"
_V2_SIDECAR = _V2_MANIFEST + ".sha256"


def _verify_path(path, label, device, owner=True):
    _owned_directory(path, label, owner)
    info = os.lstat(path)
    if device is not None:
        _require(info.st_dev == device, label + " filesystem binding")
    return info


def _exact_children(path, names, label):
    expected = set(names)
    actual = set(os.listdir(path))
    _require(len(actual) == len(expected) and actual == expected,
             label + " graph")


def _absolute_parts(path, label):
    _require(type(path) is str and path.isascii() and path.startswith("/")
             and path != "/" and all(part not in ("", ".", "..")
                                      for part in path.split("/")[1:]),
             label + " binding")
    _require(all(ord(char) >= 32 and ord(char) != 127 for char in path),
             label + " control characters")
    return path.split("/")[1:]


def _verify_bound_root():
    temp = os.environ.get("RUNNER_TEMP")
    advertised = os.environ.get("VELNOR_ROOT_RUST_CANDIDATE_ROOT")
    _absolute_parts(temp, "RUNNER_TEMP")
    expected = temp + ROOT_RELATIVE
    _require(advertised == expected, "Root Rust candidate root binding")
    current, device = "/", None
    for part in expected.split("/")[1:]:
        current += part if current == "/" else "/" + part
        owned = current == temp or current.startswith(temp + "/")
        info = _verify_path(current, "Root Rust candidate parent", device, owned)
        if current == temp:
            device = info.st_dev
        elif device is not None:
            _require(info.st_dev == device, "Root Rust candidate root filesystem binding")
    _require(device is not None, "Root Rust candidate filesystem root")
    cargo = os.environ.get("CARGO_HOME")
    rustup = os.environ.get("RUSTUP_HOME")
    _require(cargo == expected + "/cargo-home" and rustup == expected + "/rustup-home",
             "Root Rust candidate home binding")
    _verify_path(cargo, "Root Rust candidate Cargo home", device)
    _verify_path(rustup, "Root Rust candidate Rustup home", device)
    toolchains = rustup + "/toolchains"
    try:
        _verify_path(toolchains, "Root Rust candidate toolchains", device)
    except ValueError as error:
        if not error.args or not str(error).endswith(" missing"):
            raise
    else:
        _require(not os.listdir(toolchains), "Root Rust candidate toolchains must be empty")
    native = expected + "/native-dist"
    _verify_path(native, "Root Rust candidate native-dist", device)
    return expected, native, device


def _read_current(path, expected, limit, device):
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError as error:
        raise ValueError("candidate mirror file unavailable") from error
    try:
        before = os.fstat(descriptor)
        _require(stat.S_ISREG(before.st_mode) and not stat.S_ISLNK(before.st_mode)
                 and before.st_uid == os.geteuid() and before.st_nlink == 1
                 and not before.st_mode & 0o022 and before.st_dev == device,
                 "candidate mirror file shape")
        with os.fdopen(os.dup(descriptor), "rb") as stream:
            data = stream.read(limit + 1)
        _require(len(data) <= limit and len(data) == before.st_size,
                 "candidate mirror file size")
        after = os.fstat(descriptor)
        _require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                  before.st_ctime_ns, before.st_mode, before.st_uid, before.st_nlink) ==
                 (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                  after.st_ctime_ns, after.st_mode, after.st_uid, after.st_nlink)
                 and not after.st_mode & 0o022,
                 "candidate mirror file changed")
    finally:
        os.close(descriptor)
    if expected is not None:
        _digest(expected, "candidate mirror digest")
        _require(hashlib.sha256(data).hexdigest() == expected,
                 "candidate mirror SHA-256 mismatch")
    return data


def candidate_verify_mirror():
    """Verify the compiled mirror in place without downloading or publishing bytes."""
    config = globals().get("CONFIG")
    _require(config is not None, "compiled Root Rust candidate CONFIG missing")
    manifest, records = _validate_config(config)
    _, native, device = _verify_bound_root()
    dist = native + "/dist"
    dated = dist + "/" + RELEASE_DATE
    _exact_children(native, ("dist",), "Root Rust candidate native-dist")
    _verify_path(dist, "Root Rust candidate dist", device)
    _verify_path(dated, "Root Rust candidate dated dist", device)
    _exact_children(dist, (_V2_MANIFEST, _V2_SIDECAR, RELEASE_DATE),
                    "Root Rust candidate dist")
    names = tuple(_COMPONENT_FILES[component] for component in COMPONENTS)
    _exact_children(dated, names, "Root Rust candidate archive")
    manifest_data = _read_current(dist + "/" + _V2_MANIFEST,
                                  manifest["manifest_sha256"], MAX_MANIFEST_BYTES, device)
    sidecar = _read_current(dist + "/" + _V2_SIDECAR, None, MAX_MANIFEST_BYTES, device)
    expected_sidecar = (manifest["manifest_sha256"] + "  " + _V2_MANIFEST + "\n").encode("ascii")
    _require(sidecar == expected_sidecar, "candidate V2 manifest sidecar")
    _require(hashlib.sha256(manifest_data).hexdigest() == manifest["manifest_sha256"],
             "candidate V2 manifest digest")
    components = {item["component"]: item for item in manifest["components"]}
    for component in COMPONENTS:
        item = components[component]
        path = dated + "/" + _COMPONENT_FILES[component]
        archive = _read_current(path, item["xz_sha256"], MAX_ARCHIVE_BYTES, device)
        _validate_archive(archive, component, records[component])
    # The admitted native installer only reads this complete, verified mirror.
    # Runtime qualification additionally requires exclusive quiescent ownership.
    for name in names:
        os.chmod(dated + "/" + name, 0o444, follow_symlinks=False)
    for name in (_V2_MANIFEST, _V2_SIDECAR):
        os.chmod(dist + "/" + name, 0o444, follow_symlinks=False)
    for directory in (dated, dist, native):
        os.chmod(directory, 0o555, follow_symlinks=False)
