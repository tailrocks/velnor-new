"""Original prepared archive custody for qualified native Cargo verification."""
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
from types import MappingProxyType


_ORIGINAL_VERIFICATION_SEAL = object()
_VERIFICATION_MAX_FILES = 100000
_VERIFICATION_MAX_BYTES = 16 * 1024 * 1024 * 1024


def _verification_json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def original_archive_session_intent(approved, evidence, manifest):
    """Native input content only: this JSON value grants no session authority."""
    manifest = _safe_relative_manifest(manifest).as_posix()
    archives = []
    for name in evidence["publication_order"]:
        version = approved["packages"][name]
        archives.append({"package_name": name, "package_version": version,
            "archive_path": f"archives/{name}-{version}.crate",
            "sha256": evidence["packages"][name]["archive_sha256"],
            "features": [], "all_features": False, "no_default_features": False})
    return {"format": 1, "source_manifest": "snapshot/" + manifest,
            "targets": [], "archives": archives, "prepared": None}


def _verification_file(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and
                before.st_uid == os.getuid() and before.st_size <= 1024 * 1024 * 1024,
                "verification_closure_file")
        digest, total = hashlib.sha256(), 0
        while data := os.read(descriptor, 1024 * 1024):
            total += len(data)
            require(total <= 1024 * 1024 * 1024, "verification_closure_file_size")
            digest.update(data)
        after = os.fstat(descriptor)
        identity = lambda value: (value.st_dev, value.st_ino, value.st_mode, value.st_uid,
                                  value.st_nlink, value.st_size, value.st_mtime_ns,
                                  value.st_ctime_ns)
        require(identity(before) == identity(after) and total == before.st_size,
                "verification_closure_changed")
        return identity(after), digest.hexdigest()
    finally:
        os.close(descriptor)


def _verification_closure(root):
    records, total = {}, 0
    for directory, directories, files in os.walk(root, followlinks=False,
                                                onerror=_verification_walk_error):
        for name in sorted(directories + files):
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            metadata = path.lstat()
            require(not stat.S_ISLNK(metadata.st_mode), "verification_closure_symlink")
            require(len(records) < _VERIFICATION_MAX_FILES, "verification_closure_count")
            if stat.S_ISDIR(metadata.st_mode):
                records[relative] = ("directory", metadata.st_dev, metadata.st_ino,
                                     metadata.st_mode, metadata.st_uid)
            else:
                record = _verification_file(path)
                records[relative] = record
                total += record[0][5]
                require(total <= _VERIFICATION_MAX_BYTES, "verification_closure_size")
    return records


def _verification_walk_error(error):
    raise ReconcileError("verification_closure_walk") from error


class _OriginalArchiveVerificationSession:
    __slots__ = ("_seal", "_temporary", "_root", "_identity", "_prepared", "_intent_json",
                 "_originals", "_closure", "_sdk", "_phase", "_closed")

    def __init__(self, seal, prepared, manifest):
        require(seal is _ORIGINAL_VERIFICATION_SEAL, "verification_session_authority")
        authenticated_prepared_package(prepared)
        validate_prepared_evidence(prepared.evidence, prepared.approved, prepared.source)
        temporary = tempfile.TemporaryDirectory(prefix="velnor-original-verification-",
                                                dir=str(Path("/tmp").resolve(strict=True)))
        root = Path(temporary.name)
        metadata = root.lstat()
        fields = {"_seal": seal, "_prepared": prepared, "_temporary": temporary,
                  "_root": root, "_identity": (metadata.st_dev, metadata.st_ino),
                  "_sdk": None, "_closure": None, "_phase": "original", "_closed": False}
        for key, value in fields.items():
            object.__setattr__(self, key, value)
        try:
            materialize_authenticated_source_snapshot(prepared.source, self._root / "snapshot")
            guard_source_intent(str(self._root / "snapshot"))
            intent = original_archive_session_intent(prepared.approved, prepared.evidence, manifest)
            object.__setattr__(self, "_intent_json", _verification_json(intent))
            (self._root / "archives").mkdir()
            for item in intent["archives"]:
                data = prepared.archives[item["package_name"]]
                require(type(data) is bytes and hashlib.sha256(data).hexdigest() == item["sha256"],
                        "verification_original_digest")
                with (self._root / item["archive_path"]).open("xb") as output:
                    output.write(data)
            with (self._root / "verification-session.json").open("xb") as output:
                output.write(self._intent_json)
            object.__setattr__(self, "_originals", MappingProxyType(_verification_closure(self._root)))
        except BaseException:
            self._temporary.cleanup()
            raise

    def __setattr__(self, _name, _value):
        raise AttributeError("immutable_original_archive_verification_session")

    @property
    def root_path(self):
        self._require_originals_current()
        return str(self._root)

    def _require_originals_current(self):
        require(type(self) is _OriginalArchiveVerificationSession and
                getattr(self, "_seal", None) is _ORIGINAL_VERIFICATION_SEAL and
                not getattr(self, "_closed", True),
                "verification_session_authority")
        authenticated_prepared_package(self._prepared)
        authenticated_source_snapshot(self._prepared.source, 16 * 1024 * 1024)
        metadata = self._root.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and
                stat.S_IMODE(metadata.st_mode) == 0o700 and metadata.st_uid == os.getuid() and
                (metadata.st_dev, metadata.st_ino) == self._identity and
                self._root.resolve(strict=True) == self._root, "verification_root_changed")
        current = _verification_closure(self._root)
        source_paths = lambda records: {path for path in records
            if path in ("snapshot", "archives") or path.startswith(("snapshot/", "archives/"))}
        require(source_paths(current) == source_paths(self._originals),
                "verification_original_namespace_changed")
        for path, record in self._originals.items():
            if path == "verification-session.json":
                continue
            require(current.get(path) == record, "verification_original_changed")
        with (self._root / "verification-session.json").open("rb") as source:
            data = source.read(PREPARED_MAX_EVIDENCE_BYTES + 1)
        require(len(data) <= PREPARED_MAX_EVIDENCE_BYTES, "verification_session_size")
        session = decode_json(data)
        intent = decode_json(self._intent_json)
        require(type(session) is dict and set(session) == set(intent),
                "verification_session_fields")
        require(same_json({key: value for key, value in session.items() if key != "prepared"},
                          {key: value for key, value in intent.items() if key != "prepared"}),
                "verification_session_intent")
        return session, current

    def require_current(self):
        session, current = self._require_originals_current()
        require((self._phase == "prepared" and type(session["prepared"]) is dict) or
                (self._phase != "prepared" and session["prepared"] is None),
                "verification_session_phase")
        if self._closure is not None:
            require(current == self._closure, "verification_input_closure_changed")

    def require_native_verifier_sdk(self, sdk, operation):
        self.require_current()
        require(type(sdk) is NativeVerifierSdk, "verification_sdk_authority")
        sdk.require_current()
        expected = {"materialize": "original", "prepare": "materialized", "verify": "prepared"}
        require(operation in expected and self._phase == expected[operation],
                "verification_sdk_phase")
        require((operation == "materialize" and self._sdk is None) or
                (operation != "materialize" and self._sdk is sdk), "verification_sdk_identity")

    def admit_native_verifier_sdk(self, sdk):
        self.require_current()
        require(self._phase == "original" and type(sdk) is NativeVerifierSdk,
                "verification_sdk_authority")
        sdk.require_current()
        sdk._materialize_verification_inputs(self)
        self._require_originals_current()
        object.__setattr__(self, "_sdk", sdk)
        object.__setattr__(self, "_closure", MappingProxyType(_verification_closure(self._root)))
        object.__setattr__(self, "_phase", "materialized")
        self.require_current()

    def _native_preparation_complete(self):
        require(self._phase == "materialized" and type(self._sdk) is NativeVerifierSdk,
                "verification_preparation_phase")
        self._sdk.require_current()
        session, _current = self._require_originals_current()
        require(type(session["prepared"]) is dict, "verification_native_preparation_missing")
        object.__setattr__(self, "_closure", MappingProxyType(_verification_closure(self._root)))
        object.__setattr__(self, "_phase", "prepared")
        self.require_current()

    def close(self):
        if not self._closed:
            object.__setattr__(self, "_closed", True)
            self._temporary.cleanup()


def validate_original_archive_verification_session(operand):
    require(type(operand) is _OriginalArchiveVerificationSession,
            "verification_session_authority")
    operand.require_current()


def verify_source_intent():
    """No verifier outputs become authority: only qualified native success returns."""
    # The actual compiled context invokes the native loader before reading config.
    context = compiled_verification_context()
    sdk = context.native_toolcap
    require(type(sdk) is NativeVerifierSdk, "verification_sdk_authority")
    session = None
    try:
        sdk.require_current()
        prepared = load_authenticated_prepared_package()
        authenticated_prepared_package(prepared)
        require(same_json(prepared.approved, context.approved), "verification_context_policy")
        require(sdk.host == context.actual_host, "verification_context_host")
        session = _OriginalArchiveVerificationSession(_ORIGINAL_VERIFICATION_SEAL,
                                                      prepared, context.manifest)
        session.admit_native_verifier_sdk(sdk)
        sdk.prepare_original_archives(session)
        sdk.verify_original_archives(session)
        session.require_current()
    finally:
        try:
            if session is not None:
                session.close()
        finally:
            sdk.close()
