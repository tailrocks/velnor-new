"""Closed native Cargo callbacks; observation DTOs never grant execution authority.

Loaded in the immutable release namespace alongside the canonical native SDK.
Only that SDK owns operand issuance, Cargo execution and native body validation.
"""
import json

_PREPARE_CARGO_OPERATIONS = frozenset({
    "manifest_read", "full_locked_metadata", "workspace_version_lock_mutation"})
_PREPARE_CARGO_RESPONSE_FIELDS = frozenset({
    "format", "operation", "metadata", "native_context", "compiler_context",
    "observations"})
_PREPARE_CARGO_FILE_ROLES = frozenset({
    "source", "original_manifest", "normalized_manifest", "lockfile", "vcs_info"})


def _prepare_cargo_identity_observation(observation):
    """Validate diagnostic data without checking pins or conferring authority."""
    require(type(observation) is bytes and 0 < len(observation) <= 64 * 1024,
            "native_cargo_identity_bytes")
    try:
        observation.decode("utf-8")
    except UnicodeError:
        require(False, "native_cargo_identity_encoding")
    return observation


def _prepare_cargo_metadata_observation(response, operation):
    """Consume the operation's metadata after the SDK validates its native body."""
    require(type(operation) is str and operation in _PREPARE_CARGO_OPERATIONS,
            "native_cargo_operation")
    require(type(response) is dict and
            set(response) == _PREPARE_CARGO_RESPONSE_FIELDS,
            "native_cargo_response_fields")
    require(type(response["format"]) is int and response["format"] == 1 and
            type(response["operation"]) is str and response["operation"] == operation,
            "native_cargo_response_binding")
    require(type(response["metadata"]) is dict, "native_cargo_metadata")
    return response["metadata"]


def _prepare_cargo_metadata_bytes(response, operation):
    metadata = _prepare_cargo_metadata_observation(response, operation)
    try:
        return json.dumps(metadata, ensure_ascii=False, allow_nan=False).encode("utf-8")
    except (TypeError, ValueError, UnicodeError):
        require(False, "native_cargo_metadata_encoding")


def _prepare_cargo_mutation_bytes(response):
    _prepare_cargo_metadata_observation(response, "workspace_version_lock_mutation")
    contexts = response["native_context"]
    require(type(contexts) is dict and "after" in contexts and
            type(contexts["after"]) is dict and
            "governingLockContext" in contexts["after"], "native_cargo_mutation_context")
    _governing, raw = _docs_freeze_lock_observation(contexts["after"]["governingLockContext"])
    return raw


def _prepare_cargo_package_files_observation(response):
    """Project ordered archive names; only the SDK binds original source paths."""
    require(type(response) is dict and
            set(response) == {"format", "package_name", "package_version", "files"},
            "native_cargo_comparison_fields")
    require(type(response["format"]) is int and response["format"] == 1 and
            all(type(response[key]) is str and response[key] and "\x00" not in response[key]
                for key in ("package_name", "package_version")) and
            type(response["files"]) is list, "native_cargo_comparison_shape")
    paths, seen = [], set()
    for record in response["files"]:
        require(type(record) is dict and set(record) == {"path", "role", "source_path"},
                "native_cargo_comparison_record")
        path = record["path"]
        require(type(path) is str and path and "\x00" not in path and
                not path.startswith("/") and
                all(part not in ("", ".", "..") for part in path.split("/")),
                "native_cargo_comparison_path")
        try:
            path.encode("utf-8")
        except UnicodeError:
            require(False, "native_cargo_comparison_encoding")
        require(path not in seen, "native_cargo_comparison_duplicate")
        require(type(record["role"]) is str and record["role"] in _PREPARE_CARGO_FILE_ROLES,
                "native_cargo_comparison_role")
        require(record["source_path"] is None or type(record["source_path"]) is str and
                record["source_path"] and "\x00" not in record["source_path"],
                "native_cargo_comparison_source_path")
        require(record["source_path"] is not None or
                record["role"] in {"normalized_manifest", "lockfile", "vcs_info"},
                "native_cargo_comparison_source_role")
        seen.add(path)
        paths.append(path)
    return paths


class NativeCargoExecutorCallback:
    """An exact current SDK facade, with no executable or source path inputs."""
    __slots__ = ("_sdk",)

    def __init__(self, sdk):
        sdk_type = globals().get("NativeSourceSemanticsSdk")
        require(sdk_type is not None and type(sdk) is sdk_type,
                "native_cargo_sdk_origin")
        sdk.require_current()
        object.__setattr__(self, "_sdk", sdk)

    def __setattr__(self, _name, _value):
        require(False, "native_cargo_callback_immutable")

    def identity(self):
        self._sdk.require_current()
        try:
            return _prepare_cargo_identity_observation(self._sdk.cargo_identity())
        finally:
            self._sdk.require_current()

    def native_manifest_read(self):
        response = self._operation_response("manifest_read")
        return _prepare_cargo_metadata_bytes(response, "manifest_read")

    def full_locked_metadata(self, governing_lock_context):
        response = self._operation_response("full_locked_metadata", governing_lock_context)
        return _prepare_cargo_metadata_bytes(response, "full_locked_metadata")

    def workspace_version_lock_mutation(self, governing_lock_context):
        response = self._operation_response("workspace_version_lock_mutation", governing_lock_context)
        return _prepare_cargo_mutation_bytes(response)

    def package_files(self):
        self._sdk.require_current()
        try:
            operand = self._sdk.source_operand()
            response = self._sdk.comparison_read(operand)
            return _prepare_cargo_package_files_observation(response)
        finally:
            self._sdk.require_current()

    def _operation_response(self, operation, governing_lock_context=None):
        self._sdk.require_current()
        try:
            require(type(operation) is str and operation in _PREPARE_CARGO_OPERATIONS,
                    "native_cargo_operation")
            governing = None
            if operation != "manifest_read":
                governing, _raw = _docs_freeze_lock_observation(governing_lock_context)
            else:
                require(governing_lock_context is None, "native_cargo_manifest_context")
            operand = self._sdk.source_operand()
            if operation == "manifest_read":
                response = self._sdk.manifest_read(operand)
            elif operation == "full_locked_metadata":
                response = self._sdk.full_locked_metadata(operand, governing)
            else:
                response = self._sdk.workspace_version_lock_mutation(operand, governing)
            _prepare_cargo_metadata_observation(response, operation)
            return response
        finally:
            self._sdk.require_current()
