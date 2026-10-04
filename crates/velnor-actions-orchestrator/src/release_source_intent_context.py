"""Compiled preparation configuration; grants neither source nor SDK authority."""
import os
from pathlib import Path


_PREPARED_CONTEXT_SEAL = object()
_COMPILED_PREPARED_CONFIGURATION = None
_PREPARED_CONTEXT = None


class _CompiledPreparedContext:
    __slots__ = ("_approved_json", "release_config", "manifest", "actual_host",
                 "destination", "output_sink")

    def __init__(self, seal, configuration, runner_temp):
        require(seal is _PREPARED_CONTEXT_SEAL, "prepared_context_capability")
        require(type(configuration) is dict and set(configuration) == {
            "approved_json", "release_config", "manifest", "actual_host"},
            "prepared_context_fields")
        require(all(type(value) is str and value for value in configuration.values()),
                "prepared_context_values")
        approved = decode_json(configuration["approved_json"])
        require(type(approved) is dict and type(approved.get("schema")) is int and
                approved["schema"] == 1, "prepared_context_policy")
        root = Path(runner_temp)
        require(root.is_absolute() and root.resolve(strict=True) == root and root.is_dir(),
                "prepared_context_runner_temp")
        for key, value in (("_approved_json", configuration["approved_json"]),
                           ("release_config", configuration["release_config"]),
                           ("manifest", configuration["manifest"]),
                           ("actual_host", configuration["actual_host"]),
                           ("destination", root / "velnor/source-intent-prepared/prepared.zip"),
                           ("output_sink", _PreparedOutputSink(seal, root))):
            object.__setattr__(self, key, value)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_prepared_context")

    @property
    def approved(self):
        # A caller may edit its own JSON data copy, never the compiled context.
        return decode_json(self._approved_json)

    def write_prepared_payload(self, data):
        return self.output_sink.write_prepared_payload(data)


def compiled_prepared_context():
    global _PREPARED_CONTEXT
    require(_COMPILED_PREPARED_CONFIGURATION is not None, "prepared_context_unqualified")
    if _PREPARED_CONTEXT is None:
        _PREPARED_CONTEXT = _CompiledPreparedContext(
            _PREPARED_CONTEXT_SEAL, _COMPILED_PREPARED_CONFIGURATION,
            os.environ["RUNNER_TEMP"])
    return _PREPARED_CONTEXT
