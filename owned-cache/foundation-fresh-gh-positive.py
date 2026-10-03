"""Actual sealed GH public-release crypto positive; issues no cache authority."""
import hashlib
import json
import os
import sys
import tempfile

import cache_receipt_gh as gh
from cache_receipt_common import ColdReceipt
from foundation_fresh_gh_profile import current
from receipt_fresh_gh import _control, fresh_gh
from receipt_fresh_gh_download import download

_COMPILED_PUBLIC_BUNDLE = None


def _write(home, name, data):
    descriptor = os.open(home + '/' + name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(data)
    return home + '/' + name


def _result(value, record):
    if record.get('source_repository') != 'https://github.com/cli/cli':
        raise ColdReceipt('fresh_gh_positive_source_repository')
    if not isinstance(value, list) or len(value) != 1:
        raise ColdReceipt('fresh_gh_positive_count')
    result = value[0]['verificationResult']
    certificate = result['signature']['certificate']
    expected = {
        'sourceRepositoryURI': record['source_repository'],
        'sourceRepositoryDigest': record['source_commit'],
        'buildSignerDigest': record['source_commit'],
        'buildSignerURI': 'https://github.com/cli/cli/.github/workflows/deployment.yml@refs/heads/trunk',
        'sourceRepositoryVisibilityAtSigning': 'public',
        'issuer': 'https://token.actions.githubusercontent.com',
        'runnerEnvironment': 'github-hosted',
    }
    if any(certificate.get(key) != expected for key, expected in expected.items()):
        raise ColdReceipt('fresh_gh_positive_certificate')
    statement = result['statement']
    subject = {'name': record['binary_member'].split('/')[0] + '.tar.gz',
               'digest': {'sha256': record['archive_sha256']}}
    if (statement.get('_type') != 'https://in-toto.io/Statement/v1'
            or statement.get('predicateType') != 'https://slsa.dev/provenance/v1'
            or subject not in statement.get('subject', [])):
        raise ColdReceipt('fresh_gh_positive_subject')
    return value


def qualify():
    if (dict(os.environ) != {'LANG': 'C', 'LC_ALL': 'C'} or os.getcwd() != '/'
            or (sys.flags.isolated, sys.flags.no_site, sys.flags.ignore_environment,
                sys.flags.dont_write_bytecode) != (1, 1, 1, 1)):
        raise ColdReceipt('fresh_gh_positive_environment')
    profile, verifier = current(), fresh_gh()
    record = gh._COMPILED_GH_DISTRIBUTION
    try:
        with tempfile.TemporaryDirectory(prefix='gh-native-positive-', dir=_control(profile)) as home:
            # Independently fresh subject acquisition uses the same SDK owner.
            subject = _write(home, 'subject.tar.gz', download(record, profile.ca_file))
            bundle = _write(home, 'bundle.json', _COMPILED_PUBLIC_BUNDLE)
            root = _write(home, 'root.jsonl', verifier._verifier._root_bytes)
            _control(profile)
            arguments = ['attestation', 'verify', subject, '--bundle', bundle, '--repo', 'cli/cli',
                '--cert-identity',
                'https://github.com/cli/cli/.github/workflows/deployment.yml@refs/heads/trunk',
                '--signer-digest', record['source_commit'], '--source-ref', 'refs/heads/trunk',
                '--source-digest', record['source_commit'], '--predicate-type',
                'https://slsa.dev/provenance/v1', '--cert-oidc-issuer',
                'https://token.actions.githubusercontent.com', '--deny-self-hosted-runners',
                '--custom-trusted-root', root, '--format', 'json']
            output = _result(gh._run(verifier._verifier._descriptor, arguments, home), record)
            _control(profile)
        return {'schema': 1, 'authority': False, 'state': 'fresh-gh-native-positive',
                'scope': 'initial-fresh-sdk-and-public-release-verifier-compatibility-only',
                'distribution': record, 'sealed_execution': True,
                'root_sha256': hashlib.sha256(verifier._verifier._root_bytes).hexdigest(),
                'bundle_sha256': hashlib.sha256(_COMPILED_PUBLIC_BUNDLE).hexdigest(),
                'verification': output}
    finally:
        verifier.close()


print(json.dumps(qualify(), separators=(',', ':'), allow_nan=False))
