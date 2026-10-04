"""Private original SourceJob observations; the actual origin issuer is absent."""


def _compiled_original_source_context():
    # Qualified workflow/run/attempt/SourceJob authentication is not installed.
    return None


def _original_source_identity_values(value):
    """Check closed identity data only. Equality never issues source authority."""
    fields = ("repository_id",) + tuple(sorted(_SOURCE_DESCRIPTOR_FIELDS))
    require(type(value) is tuple and len(value) == len(fields) and
            all(type(item) is tuple and len(item) == 2 and
                type(item[0]) is str for item in value) and
            tuple(item[0] for item in value) == fields,
            "original_source_identity_fields")
    repository_id = value[0][1]
    require(type(repository_id) is int and 0 < repository_id <= (1 << 64) - 1,
            "original_source_repository_id")
    descriptor = dict(value[1:])
    validate_source_snapshot_descriptor(descriptor)
    return repository_id, descriptor


def _original_source_identity_observation(source, repository_id):
    """Retain original-cap observations. Caller repository ID remains data."""
    descriptor = authenticated_source_descriptor(source)
    value = (("repository_id", repository_id),) + tuple(sorted(descriptor.items()))
    _original_source_identity_values(value)
    return value


def _compare_original_source_identities(expected, authenticated):
    # Native ownership must independently authenticate both origins before use.
    _original_source_identity_values(expected)
    _original_source_identity_values(authenticated)
    require(expected == authenticated, "original_source_identity_mismatch")


def _check_original_source_repository_response(expected, response):
    """Pure response comparison; a response mapping is not authenticated origin."""
    repository_id, descriptor = _original_source_identity_values(expected)
    require(type(response) is dict and type(response.get("id")) is int and
            response["id"] == repository_id and
            type(response.get("full_name")) is str and
            response.get("full_name") == descriptor["repository"],
            "original_source_repository_identity")


def authenticate_original_source_origin():
    # No artifact read, repository GET, materialization, or FD duplication occurs
    # before the genuine service-context owner exists. Substitutions also deny.
    context = _compiled_original_source_context()
    require(context is not None, "original_source_service_context_missing")
    require(False, "original_source_service_context_unqualified")


def load_original_source_job_owned():
    authenticate_original_source_origin()
