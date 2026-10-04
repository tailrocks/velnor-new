"""Compiled verification configuration; the dedicated native owner grants tools."""

_VERIFICATION_CONTEXT_SEAL = object()
_COMPILED_VERIFICATION_CONFIGURATION = None
_VERIFICATION_CONTEXT = None


class _CompiledVerificationContext:
    __slots__ = ("_approved_json", "manifest", "actual_host", "native_toolcap")

    def __init__(self, seal, configuration, native_toolcap):
        require(seal is _VERIFICATION_CONTEXT_SEAL, "verification_context_capability")
        require(type(configuration) is dict and set(configuration) == {
            "approved_json", "manifest", "actual_host"}, "verification_context_fields")
        require(all(type(value) is str and value for value in configuration.values()),
                "verification_context_values")
        require(configuration["actual_host"] == "x86_64-unknown-linux-gnu",
                "verification_context_host")
        approved = decode_json(configuration["approved_json"])
        require(type(approved) is dict and type(approved.get("schema")) is int and
                approved["schema"] == 1, "verification_context_policy")
        for key, value in (("_approved_json", configuration["approved_json"]),
                           ("manifest", configuration["manifest"]),
                           ("actual_host", configuration["actual_host"]),
                           ("native_toolcap", native_toolcap)):
            object.__setattr__(self, key, value)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_verification_context")

    @property
    def approved(self):
        return decode_json(self._approved_json)


def compiled_verification_context():
    global _VERIFICATION_CONTEXT
    sdk_type = globals().get("NativeVerifierSdk")
    loader = globals().get("load_native_verifier_sdk")
    require(callable(sdk_type) and callable(loader), "verification_native_owner_unavailable")
    if _VERIFICATION_CONTEXT is None:
        # The actual dedicated owner rejects missing distribution/kernel authority
        # before configuration parsing, source loading, or context construction.
        native_toolcap = loader()
        require(type(native_toolcap) is sdk_type, "verification_native_owner_capability")
        try:
            native_toolcap.require_current()
            require(_COMPILED_VERIFICATION_CONFIGURATION is not None,
                    "verification_context_unqualified")
            _VERIFICATION_CONTEXT = _CompiledVerificationContext(
                _VERIFICATION_CONTEXT_SEAL, _COMPILED_VERIFICATION_CONFIGURATION,
                native_toolcap)
        except BaseException:
            native_toolcap.close()
            raise
    try:
        _VERIFICATION_CONTEXT.native_toolcap.require_current()
    except BaseException:
        failed = _VERIFICATION_CONTEXT
        _VERIFICATION_CONTEXT = None
        failed.native_toolcap.close()
        raise
    return _VERIFICATION_CONTEXT
