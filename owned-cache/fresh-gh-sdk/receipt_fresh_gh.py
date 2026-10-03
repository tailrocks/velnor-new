"""Source-only fresh verifier factory; native Foundation proof gates activation.

The profile is issued by the existing tools-cache Foundation owner, never JSON,
environment, cache payload or a caller Boolean. Official GH release hashes come
solely from the SDK projection embedded in cache_receipt_gh.
"""
import os
import tempfile

import cache_receipt_gh as gh
from cache_receipt_common import ColdReceipt
from receipt_fresh_gh_archive import extract
from receipt_fresh_gh_download import download

_COMPILED_TRUSTED_ROOT = None
_COMPILED_REPOSITORY = None


def _qualified_foundation():
    # No real Linux Python stdlib/native-extension/loader/CA profile is issued.
    # A tools-cache source factory must replace this whole source decision after
    # native proof; observations and runtime profile documents cannot enable it.
    return None


def _control(profile):
    profile.require_current()
    root = profile.control_root
    gh._root(root)
    if not profile.payload_roots:
        raise ColdReceipt('fresh_gh_payload_roots')
    for payload in profile.payload_roots:
        if (not isinstance(payload, str) or not payload.startswith('/')
                or os.path.normpath(payload) != payload):
            raise ColdReceipt('fresh_gh_payload_root')
        for compared in (payload, os.path.realpath(payload)):
            common = os.path.commonpath((root, compared))
            if common in (root, compared):
                raise ColdReceipt('fresh_gh_control_overlap')
    return root


class _FreshGh(gh.QualifiedGh):
    """Recheck complete Foundation authority before each native execution."""
    def __init__(self, verifier, profile):
        self._verifier = verifier
        self._profile = profile

    def close(self):
        self._verifier.close()

    def verify(self, bundle_bytes, manifest_bytes, policy):
        _control(self._profile)
        result = self._verifier.verify(bundle_bytes, manifest_bytes, policy)
        _control(self._profile)
        return result

    def api(self, endpoint, paginate=False):
        _control(self._profile)
        result = self._verifier.api(endpoint, paginate=paginate)
        _control(self._profile)
        return result


def fresh_gh():
    """No caller input; root/repository are compiled qualified-owner literals."""
    profile = _qualified_foundation()
    if profile is None:
        raise ColdReceipt('fresh_gh_foundation_unqualified')
    root_bytes, repository = _COMPILED_TRUSTED_ROOT, _COMPILED_REPOSITORY
    if (not isinstance(root_bytes, bytes) or not 0 < len(root_bytes) <= 65536
            or not isinstance(repository, str)
            or gh.re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository) is None):
        raise ColdReceipt('fresh_gh_source_binding_unqualified')
    record = gh._COMPILED_GH_DISTRIBUTION
    if record is None:
        raise ColdReceipt('gh_distribution_unqualified')
    if (os.uname().sysname != 'Linux' or record.get('machine') != os.uname().machine
            or record.get('tool') != 'gh' or record.get('version') != '2.102.0'
            or not all(gh._digest(record.get(key)) for key in
                       ('archive_sha256', 'binary_sha256', 'qualification_sha256'))):
        raise ColdReceipt('fresh_gh_distribution')
    root = _control(profile)
    verifier = None
    try:
        with tempfile.TemporaryDirectory(prefix='velnor-fresh-gh-', dir=root) as home:
            archive = download(record, profile.ca_file)
            _control(profile)
            path = extract(archive, record, home)
            _control(profile)
            verifier = gh.qualified_gh(path, root_bytes, root, repository)
        return _FreshGh(verifier, profile)
    except BaseException:
        if verifier is not None:
            verifier.close()
        raise
