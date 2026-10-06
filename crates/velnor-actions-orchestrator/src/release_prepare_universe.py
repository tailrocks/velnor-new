"""Bounded frozen local-registry primitives; native coverage activation is guarded.

These bytes alone cannot authorize BaselineLockResolution. Cargo's resolver must
own source-qualified request coverage before the absent-lock route can use them.
No dependency traversal, version choice, or caller completeness flag lives here.
"""
import stat
import os


UNIVERSE_PACKAGE_LIMIT = 256
UNIVERSE_BYTE_LIMIT = 256 * 1024 * 1024
UNIVERSE_MEMBER_LIMIT = 100000
UNIVERSE_SOURCE_ID = "registry+https://github.com/rust-lang/crates.io-index"


def universe_index_records(raw, name):
    """Validate identities while preserving the entire solver index response."""
    require(isinstance(raw, bytes) and 0 < len(raw) <= REGISTRY_INDEX_LIMIT,
            "universe_index_size")
    records = {}
    try:
        for line in raw.decode("utf-8").splitlines():
            entry = json.loads(line, object_pairs_hook=registry_unique_object,
                               parse_constant=lambda _: require(False, "registry_json_constant"))
            require(isinstance(entry, dict) and entry.get("name") == name,
                    "universe_index_identity")
            version = entry.get("vers")
            _require_version(version)
            require(version not in records and isinstance(entry.get("cksum"), str) and
                    re.fullmatch(r"[0-9a-f]{64}", entry["cksum"]) and
                    type(entry.get("yanked")) is bool, "universe_index_identity")
            records[version] = entry["cksum"]
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ReconcileError("universe_index_decode") from error
    require(records, "universe_index_empty")
    return records


def universe_paths(root):
    """Error-reporting traversal: never turn unreadable entries into absence."""
    def unreadable(error):
        raise ReconcileError("universe_source_unreadable") from error

    yield root
    for directory, directories, files in os.walk(root, onerror=unreadable, followlinks=False):
        for name in sorted(directories + files):
            path = Path(directory) / name
            require(not path.is_symlink(), "universe_source_type")
            yield path


def universe_tree(root):
    """Bind every directory, file byte, mode and identity; reject links/cache marks."""
    require(root.is_dir() and not root.is_symlink() and
            all(not parent.is_symlink() for parent in root.parents), "universe_source_path")
    inventory, total = {}, 0
    for path in universe_paths(root):
        require(len(inventory) < UNIVERSE_MEMBER_LIMIT, "universe_source_count")
        try:
            metadata = path.lstat()
        except OSError as error:
            raise ReconcileError("universe_source_unreadable") from error
        mode = stat.S_IMODE(metadata.st_mode)
        require(stat.S_ISREG(metadata.st_mode) or stat.S_ISDIR(metadata.st_mode),
                "universe_source_type")
        require(path.name != ".cargo-ok", "universe_source_cache_marker")
        require(mode & 0o444 and (not stat.S_ISDIR(metadata.st_mode) or mode & 0o111),
                "universe_source_unreadable")
        relative = path.relative_to(root).as_posix()
        if stat.S_ISREG(metadata.st_mode):
            require(metadata.st_nlink == 1, "universe_source_hardlink")
            total += metadata.st_size
            require(total <= UNIVERSE_BYTE_LIMIT, "universe_source_size")
            try:
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
            except OSError as error:
                raise ReconcileError("universe_source_unreadable") from error
        else:
            digest = None
        inventory[relative] = (metadata.st_dev, metadata.st_ino, mode, digest)
    return inventory


class FrozenRegistrySnapshot:
    """Integrity snapshot only; deliberately carries no resolver authority."""

    def __init__(self, root, inventory, index, archives):
        self._root, self._inventory = root, inventory
        self._index, self._archives = dict(index), dict(archives)

    @property
    def local_registry(self):
        return self._root

    def verify(self):
        require(universe_tree(self._root) == self._inventory, "universe_source_changed")

    def native_registry_entry(self):
        """Canonical native ABI data from fetched bytes; carries no coverage authority."""
        self.verify()
        return {"source_id": UNIVERSE_SOURCE_ID, "root": str(self._root),
                "index": dict(self._index), "archives": dict(self._archives)}


class FrozenRegistryMaterializer:
    """Acquire fixed registry bytes, never infer the resolver request universe."""

    def __init__(self, destination, fetch=None):
        self._root = Path(destination).absolute()
        require(not self._root.exists() and not self._root.is_symlink() and
                all(not parent.is_symlink() for parent in self._root.parents),
                "universe_destination")
        self._root.mkdir(parents=True)
        (self._root / "index").mkdir()
        self._fetch = registry_fetch if fetch is None else fetch
        self._indices, self._archives, self._bytes, self._frozen = {}, set(), 0, False
        self._index_observations, self._archive_observations = {}, {}
        self._inventory = universe_tree(self._root)

    def _mutable(self):
        require(not self._frozen, "universe_already_frozen")
        require(universe_tree(self._root) == self._inventory, "universe_source_changed")

    def _reserve(self, raw):
        require(isinstance(raw, bytes), "universe_response_type")
        require(self._bytes + len(raw) <= UNIVERSE_BYTE_LIMIT, "universe_source_size")
        self._bytes += len(raw)

    def fetch_index(self, name):
        """Retain all candidates, unknown Cargo fields, whitespace and line endings."""
        self._mutable()
        require(isinstance(name, str) and REGISTRY_NAME.fullmatch(name), "registry_name")
        require(name not in self._indices and len(self._indices) < UNIVERSE_PACKAGE_LIMIT,
                "universe_index_duplicate_or_count")
        relative = registry_index_path(name)
        path = self._root / "index" / relative
        require(not path.exists() and relative not in self._index_observations,
                "universe_index_path_collision")
        url = "https://index.crates.io/" + relative
        try:
            raw = self._fetch(url, REGISTRY_INDEX_LIMIT)
        except RegistryNotFound as error:
            require(error.url == url, "universe_negative_identity")
            self._mutable()
            self._indices[name] = None
            self._index_observations[relative] = None
            return None
        self._mutable()
        records = universe_index_records(raw, name)
        self._reserve(raw)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as output:
            output.write(raw)
        self._indices[name] = records
        self._index_observations[relative] = hashlib.sha256(raw).hexdigest()
        self._inventory = universe_tree(self._root)
        return path

    def fetch_archive(self, name, version):
        """Acquire one requested archive bound to a previously fetched index entry."""
        self._mutable()
        _require_version(version)
        require(isinstance(name, str) and isinstance(self._indices.get(name), dict) and
                version in self._indices[name],
                "universe_archive_unknown_index")
        require((name, version) not in self._archives and
                len(self._archives) < UNIVERSE_PACKAGE_LIMIT, "universe_archive_duplicate_or_count")
        prefix = f"{name}-{version}"
        raw = self._fetch(f"https://static.crates.io/crates/{name}/{prefix}.crate",
                          REGISTRY_ARCHIVE_LIMIT)
        self._mutable()
        require(isinstance(raw, bytes) and len(raw) <= REGISTRY_ARCHIVE_LIMIT and
                hashlib.sha256(raw).hexdigest() == self._indices[name][version],
                "registry_archive_checksum")
        staging = Path(tempfile.mkdtemp(prefix=".universe-extract-", dir=self._root.parent))
        try:
            registry_extract(raw, staging, prefix)
            registry_bind_manifest(staging / prefix, name, version)
        finally:
            shutil.rmtree(staging)
        self._reserve(raw)
        path = self._root / (prefix + ".crate")
        with path.open("xb") as output:
            output.write(raw)
        self._archives.add((name, version))
        self._archive_observations[path.name] = hashlib.sha256(raw).hexdigest()
        self._inventory = universe_tree(self._root)
        return path

    def freeze(self):
        """Seal a local-registry layout; a missing selected archive remains a hard fail."""
        self._mutable()
        require(self._indices, "universe_source_empty")
        for relative in sorted(self._inventory, reverse=True):
            path = self._root / relative
            path.chmod(0o555 if path.is_dir() else 0o444)
        self._root.chmod(0o555)
        self._frozen = True
        return FrozenRegistrySnapshot(self._root, universe_tree(self._root),
                                      self._index_observations, self._archive_observations)
