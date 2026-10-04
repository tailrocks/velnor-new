"""Synthetic readonly registry seeds exercise the checked-in copy routes."""

import importlib.util
from pathlib import Path
import stat
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUN = load("readonly_seed_run", "run.py")
T08 = load("readonly_seed_t08", "run_negative_t08_v2.py")
BASE = RUN
IDENTITY = "index.crates.io-1949cf8c6b5b557f"


def physical(root):
    records = []
    for path in [root, *sorted(root.rglob("*"))]:
        if path.is_symlink():
            raise AssertionError("unexpected registry seed symlink")
        info = path.stat()
        records.append((path.relative_to(root).as_posix(), stat.S_IMODE(info.st_mode),
                        BASE.digest(path) if path.is_file() else None, info.st_ino))
    return records


class ReadonlyRegistrySeedTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.root = Path(temporary.name).resolve()

        def cleanup():
            for path in self.root.rglob("*"):
                path.chmod(0o700 if path.is_dir() else 0o600)
            temporary.cleanup()

        self.addCleanup(cleanup)
        self.seed = self.root / "readonly-seed"
        self.archive18 = Path("registry/cache") / IDENTITY / "itoa-1.0.18.crate"
        self.source18 = Path("registry/src") / IDENTITY / "itoa-1.0.18"
        self.archive17 = Path("registry/cache") / IDENTITY / "itoa-1.0.17.crate"
        self.source17 = Path("registry/src") / IDENTITY / "itoa-1.0.17"
        self.write(self.archive18, b"synthetic itoa 1.0.18 archive")
        self.write(self.source18 / "Cargo.toml", b"name = 'itoa'\nversion = '1.0.18'\n")
        self.write(self.source18 / "src/lib.rs", b"pub fn format() {}\n")
        self.write(self.source18 / ".cargo-ok", b"ok\n")
        self.write(Path("registry/index") / IDENTITY / "config.json", b"{}\n")
        self.write(Path("registry/index") / IDENTITY / ".cache/it/oa/itoa", b"synthetic index\n")
        self.write(self.archive17, b"synthetic itoa 1.0.17 archive")
        self.write(self.source17 / "Cargo.toml", b"name = 'itoa'\nversion = '1.0.17'\n")
        self.write(self.source17 / "src/lib.rs", b"pub fn format() {}\n")
        self.write(self.source17 / ".cargo-ok", b"ok\n")
        for path in [self.seed, *self.seed.rglob("*")]:
            path.chmod(0o500 if path.is_dir() else 0o400)
        self.before = physical(self.seed)

    def write(self, relative, data):
        path = self.seed / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def copy_arguments(self, dual=False):
        values = dict(registry_home=self.seed, archive_relative=self.archive18,
                      source_relative=self.source18)
        if dual:
            values.update(registry17_archive=self.seed / self.archive17,
                          registry17_source=self.seed / self.source17)
        return type("SeedArgs", (), values)()

    def assert_copy(self, destination, include17):
        source_files = BASE.BIND.inventory(self.seed / self.source18)
        source_prefix = self.source18.relative_to(Path("registry"))
        expected = [dict(item, path=(source_prefix / item["path"]).as_posix())
                    for item in source_files]
        copied = [self.archive18,
                  Path("registry/index") / IDENTITY / "config.json",
                  Path("registry/index") / IDENTITY / ".cache/it/oa/itoa"]
        expected.extend(dict(path=path.relative_to(Path("registry")).as_posix(),
                             size=(self.seed / path).stat().st_size,
                             sha256=BASE.digest(self.seed / path)) for path in copied)
        if include17:
            source_prefix = self.source17.relative_to(Path("registry"))
            expected.extend(dict(item, path=(source_prefix / item["path"]).as_posix())
                            for item in BASE.BIND.inventory(self.seed / self.source17))
            archive_path = self.archive17.relative_to(Path("registry"))
            expected.append(dict(path=archive_path.as_posix(),
                                 size=(self.seed / self.archive17).stat().st_size,
                                 sha256=BASE.digest(self.seed / self.archive17)))
        expected.append(dict(path="CACHEDIR.TAG", size=len(BASE.CARGO_CACHE_TAG),
                             sha256=BASE.BIND.sha(BASE.CARGO_CACHE_TAG)))
        actual = BASE.tree(destination / "registry")
        self.assertEqual(actual, sorted(expected, key=lambda item: item["path"]))
        tag = destination / "registry/CACHEDIR.TAG"
        self.assertEqual(tag.read_bytes(), BASE.CARGO_CACHE_TAG)
        self.assertEqual(physical(self.seed), self.before)

    def test_checked_in_seed_copy_preserves_synthetic_readonly_input_bytes(self):
        destination = self.root / "cargo-home"
        destination.mkdir(mode=0o700)
        BASE.seed_registry(self.copy_arguments(), destination)
        self.assert_copy(destination, include17=False)

    def test_checked_in_dual_seed_copy_preserves_both_version_inputs(self):
        destination = self.root / "dual-cargo-home"
        destination.mkdir(mode=0o700)
        T08.seed_registry(self.copy_arguments(dual=True), destination)
        self.assert_copy(destination, include17=True)


if __name__ == "__main__":
    unittest.main()
