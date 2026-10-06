"""Native APT source coherence verification. Downloaded programs never execute."""
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
from delivery_apt_core import (config_digest, digest, loads, read_bytes, read_json,
                               regular, require, run, write_json, deb_payload, elf_identity)

ARCHES = ('amd64', 'arm64')
INCOMING = Path('incoming')


def hex_value(value, length):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(length) + '}', value) is not None


def equal(document, expected):
    require(isinstance(document, dict), 'JSON object required')
    for key, value in expected.items():
        require(document.get(key) == value, 'identity mismatch: ' + key)


def version_parts(suite, version):
    if suite == 'stable':
        require(re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', version), 'stable version must be vX.Y.Z')
        return version[1:], version[1:]
    match = re.fullmatch(r'([0-9]+\.[0-9]+\.[0-9]+)~preview\.[0-9]+\+([0-9a-f]{7})', version)
    require(match, 'preview version must be X.Y.Z~preview.N+7hex')
    return version, match.group(1)


def validate_config(config):
    parts = config['source_repository'].split('/')
    require(len(parts) == 2, 'invalid source repository')
    owner, repo = parts
    require(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9-]{0,38}', owner) and
            not owner.endswith('-') and '--' not in owner, 'invalid source owner')
    require(len(repo) <= 100 and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', repo)
            and '..' not in repo, 'invalid source repository name')
    require(re.fullmatch(r'[a-z0-9][a-z0-9+.-]+', config['package']), 'unsafe package')
    for key in ('binary', 'identity_directory'):
        require(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', config[key]) and '..' not in config[key], 'unsafe ' + key)
    keyring = PurePosixPath(config['keyring'])
    require(not keyring.is_absolute() and all(part and part not in ('.', '..')
            and not part.startswith('-') for part in config['keyring'].split('/')), 'unsafe keyring')
    require(re.fullmatch(r'[A-Za-z0-9_./-]+', config['keyring']), 'unsafe keyring')
    require(re.fullmatch('[0-9A-F]{40}', config['signer_fingerprint']), 'invalid signer fingerprint')
    require(re.fullmatch(r'\.github/workflows/[A-Za-z0-9][A-Za-z0-9_.-]*\.yml', config['signer_workflow']), 'unsafe signer workflow')
    require('..' not in config['signer_workflow'], 'unsafe signer workflow')
    require(re.fullmatch(r'ghcr\.io/[a-z0-9][a-z0-9._-]*(/[a-z0-9][a-z0-9._-]*)+',
                         config['oci_image_repository']) and
            all(part not in ('.', '..') for part in config['oci_image_repository'].split('/')),
            'unsafe OCI image repository')
    require(re.fullmatch(r'\.github/workflows/[A-Za-z0-9][A-Za-z0-9_.-]*\.yml',
                         config['oci_signer_workflow']) and '..' not in config['oci_signer_workflow'],
            'unsafe OCI signer workflow')


def signer(config):
    regular(config['keyring'])
    text = run(['gpg', '--batch', '--with-colons', '--show-keys', config['keyring']])
    fingerprints, primary = [], False
    for line in text.splitlines():
        parts = line.split(':')
        if parts[0] == 'pub':
            primary = True
        elif parts[0] == 'sub':
            primary = False
        elif parts[0] == 'fpr' and primary:
            require(len(parts) > 9, 'invalid GPG fingerprint record')
            fingerprints.append(parts[9])
            primary = False
    require(fingerprints == [config['signer_fingerprint']], 'keyring signer does not match pinned fingerprint')


def download(config, tag, names):
    argv = ['gh', 'release', 'download', tag, '--repo', config['source_repository'], '--dir', str(INCOMING)]
    for name in sorted(names):
        require('/' not in name and name not in ('.', '..'), 'unsafe asset name')
        argv.extend(['--pattern', name])
    run(argv)
    for name in names:
        regular(INCOMING / name)


def resolve_commit(config, tag):
    repo = config['source_repository']
    obj = loads(run(['gh', 'api', 'repos/' + repo + '/git/ref/tags/' + tag]))['object']
    for unused in range(16):
        require(hex_value(obj.get('sha'), 40), 'invalid tag object digest')
        if obj.get('type') == 'commit':
            return obj['sha']
        require(obj.get('type') == 'tag', 'tag does not identify a commit')
        obj = loads(run(['gh', 'api', 'repos/' + repo + '/git/tags/' + obj['sha']]))['object']
    raise ValueError('annotated tag chain exceeds limit')


def checksum(name):
    text = read_bytes(INCOMING / (name + '.sha256')).decode('utf-8')
    require(len(text.splitlines()) == 1, 'checksum sidecar must contain one line')
    fields = text.split()
    require(len(fields) in (1, 2) and hex_value(fields[0], 64), 'invalid checksum sidecar')
    require(len(fields) == 1 or fields[1] == name, 'checksum sidecar names another asset')
    actual = digest(INCOMING / name)
    require(fields[0] == actual, 'checksum mismatch: ' + name)
    return actual


def attest(config, name, commit, ref):
    run(['gh', 'attestation', 'verify', str(INCOMING / name), '--repo', config['source_repository'],
         '--source-digest', commit, '--source-ref', ref, '--signer-workflow',
         config['source_repository'] + '/' + config['signer_workflow'], '--deny-self-hosted-runners'])


def deb_identity(config, name, arch, version, base, commit, row=None, manifest_hash=None):
    for key, wanted in [('Package', config['package']), ('Version', version), ('Architecture', arch)]:
        actual = run(['dpkg-deb', '-f', str(INCOMING / name), key]).strip()
        require(actual == wanted, 'deb control mismatch: ' + key)
    files = deb_payload(INCOMING / name)
    root = 'usr/share/' + config['identity_directory'] + '/'
    require(root + 'build-identity.json' in files, 'deb missing build identity')
    identity = loads(files[root + 'build-identity.json'].decode('utf-8'))
    equal(identity, {'source_sha': commit, 'crate_version': base})
    daemon = 'usr/bin/' + config['binary']
    require(daemon in files, 'deb missing binary')
    elf_identity(files[daemon], arch)
    if row is not None:
        require(root + 'manifest.json' in files, 'deb missing manifest')
        require(hashlib.sha256(files[root + 'manifest.json']).hexdigest() == manifest_hash, 'packaged manifest mismatch')
        require(hashlib.sha256(files[daemon]).hexdigest() == row.get('binary_sha256'), 'packaged binary mismatch')


def architecture_rows(record):
    rows = record.get('architectures')
    require(isinstance(rows, list) and len(rows) == 2, 'record requires two architectures')
    require(all(isinstance(row, dict) for row in rows), 'invalid architecture row')
    require(sorted(row.get('arch', '') for row in rows) == list(ARCHES), 'architecture set mismatch')
    targets = {'amd64': 'x86_64-unknown-linux-gnu', 'arm64': 'aarch64-unknown-linux-gnu'}
    for row in rows:
        require(row.get('target') == targets[row['arch']], 'architecture target mismatch')
    return {row['arch']: row for row in rows}


def inspect(image):
    return loads(run(['docker', 'buildx', 'imagetools', 'inspect', image, '--format', '{{json .}}']))


def oci_verify(config, record, rows, version, commit, manifest_hash):
    index_digest = record.get('oci_index_digest', '')
    require(index_digest.startswith('sha256:') and hex_value(index_digest[7:], 64), 'invalid OCI index digest')
    image = record.get('oci_image_ref', '')
    require(isinstance(image, str) and image.endswith('@' + index_digest), 'OCI reference not pinned')
    image_repo = image.split('@')[0]
    require(image_repo == config['oci_image_repository'], 'OCI repository differs from trusted policy')
    run(['gh', 'attestation', 'verify', 'oci://' + image, '--repo', config['source_repository'],
         '--source-digest', commit, '--source-ref', 'refs/tags/v' + version, '--signer-workflow',
         config['source_repository'] + '/' + config['oci_signer_workflow'], '--deny-self-hosted-runners'])
    source = 'https://github.com/' + config['source_repository']
    equal(record.get('oci_labels'), {'version': version, 'revision': commit, 'source': source,
                                    'manifest_sha256': manifest_hash})
    index = inspect(image).get('manifest', {})
    require(index.get('digest') == index_digest, 'live OCI index digest mismatch')
    children = index.get('manifests')
    require(isinstance(children, list), 'live OCI child list missing')
    labels_expected = {'org.opencontainers.image.version': version,
                       'org.opencontainers.image.revision': commit,
                       'org.opencontainers.image.source': source,
                       'org.velnor.manifest-sha256': manifest_hash}
    for arch, row in rows.items():
        child_digest = row.get('oci_platform_digest', '')
        require(child_digest.startswith('sha256:') and hex_value(child_digest[7:], 64), 'invalid OCI child digest')
        matches = [item for item in children if item.get('digest') == child_digest
                   and item.get('platform', {}).get('os') == 'linux'
                   and item.get('platform', {}).get('architecture') == arch]
        require(len(matches) == 1, 'live OCI platform mismatch')
        child = inspect(image_repo + '@' + child_digest)
        require(child.get('manifest', {}).get('digest') == child_digest, 'live OCI child digest mismatch')
        equal(child.get('image', {}).get('config', {}).get('Labels'), labels_expected)


def stable(config, version, commit):
    debian, base = version_parts('stable', version)
    resolved = resolve_commit(config, version)
    require(not commit or commit == resolved, 'input commit differs from resolved tag')
    commit, ref = resolved, 'refs/tags/' + version
    names = [config['package'] + '-' + debian + '-' + arch + '.deb' for arch in ARCHES]
    files = ['release-record.json', 'manifest.json', *names]
    download(config, version, files + [name + '.sha256' for name in files])
    record_hash, manifest_hash = checksum('release-record.json'), checksum('manifest.json')
    record, manifest = read_json(INCOMING / 'release-record.json'), read_json(INCOMING / 'manifest.json')
    equal(record, {'schema': 'velnor.release-record/v1'})
    build = record.get('build')
    equal(build, {'repository': config['source_repository'], 'tag': version, 'crate_version': base,
                  'debian_version': debian, 'commit': commit, 'manifest_sha256': manifest_hash})
    require(type(build.get('manifest_version')) is int and build['manifest_version'] > 0, 'manifest version must be positive integer')
    equal(manifest, {'source_sha': commit, 'crate_version': base, 'version': build['manifest_version']})
    rows = architecture_rows(record)
    oci_verify(config, record, rows, debian, commit, manifest_hash)
    packages = []
    for arch, name in zip(ARCHES, names):
        sha = checksum(name)
        require(rows[arch].get('deb_sha256') == sha, 'record deb digest mismatch')
        attest(config, name, commit, ref)
        deb_identity(config, name, arch, debian, base, commit, rows[arch], manifest_hash)
        packages.append({'arch': arch, 'name': name, 'sha256': sha})
    return commit, ref, packages, record_hash, manifest_hash


def preview(config, version, commit):
    download(config, 'preview', ['release-manifest.json'])
    manifest = read_json(INCOMING / 'release-manifest.json')
    require(isinstance(manifest, dict), 'preview manifest must be object')
    version = version or manifest.get('version', '')
    commit = commit or manifest.get('source_commit', '')
    debian, base = version_parts('preview', version)
    require(hex_value(commit, 40) and version.endswith('+' + commit[:7]), 'preview commit mismatch')
    ref = 'refs/heads/main'
    equal(manifest, {'schema': config['manifest_schema'], 'source_repository': config['source_repository'],
                     'source_ref': ref, 'source_commit': commit, 'version': version})
    assets = manifest.get('assets')
    require(isinstance(assets, list) and len(assets) == 2, 'preview requires exactly two assets')
    names = [config['package'] + '-preview-' + version.replace('~', '.') + '-' + arch + '.deb' for arch in ARCHES]
    download(config, 'preview', ['SHA256SUMS', *names, *[name + '.sha256' for name in names]])
    sums = [line.split() for line in read_bytes(INCOMING / 'SHA256SUMS').decode('utf-8').splitlines() if line.strip()]
    require(len(sums) == 2 and all(len(row) == 2 and hex_value(row[0], 64) for row in sums), 'invalid preview SHA256SUMS')
    packages = []
    for arch, name in zip(ARCHES, names):
        sha = checksum(name)
        accepted = (name, name.replace('.preview.', '~preview.', 1))
        require(sum(row[0] == sha and row[1] in accepted for row in sums) == 1, 'preview checksum list mismatch')
        require(sum(isinstance(asset, dict) and asset.get('sha256') == sha and asset.get('name') in accepted
                    for asset in assets) == 1, 'preview manifest asset mismatch')
        attest(config, name, commit, ref)
        deb_identity(config, name, arch, debian, base, commit)
        packages.append({'arch': arch, 'name': name, 'sha256': sha})
    return version, commit, ref, packages, digest(INCOMING / 'release-manifest.json')


def verify(config):
    validate_config(config)
    consumer_sha = os.environ.get('GITHUB_SHA', '')
    require(hex_value(consumer_sha, 40), 'invalid consumer source SHA')
    suite = os.environ.get('CHANNEL', 'stable')
    require(suite in ('stable', 'preview'), 'CHANNEL must be stable or preview')
    version, commit = os.environ.get('INPUT_VERSION', ''), os.environ.get('INPUT_COMMIT', '')
    require(not commit or hex_value(commit, 40), 'invalid input commit')
    require(not INCOMING.exists() and not INCOMING.is_symlink(), 'incoming must be absent before verification')
    signer(config)
    INCOMING.mkdir(mode=0o700)
    if suite == 'stable':
        if not version:
            releases = loads(run(['gh', 'release', 'list', '--repo', config['source_repository'],
                                 '--exclude-drafts', '--exclude-pre-releases', '--limit', '1', '--json', 'tagName']))
            require(isinstance(releases, list) and len(releases) == 1, 'no stable source release')
            version = releases[0]['tagName']
        commit, ref, packages, record_hash, manifest_hash = stable(config, version, commit)
    else:
        version, commit, ref, packages, manifest_hash = preview(config, version, commit)
        record_hash = manifest_hash
    debian, unused = version_parts(suite, version)
    files = {path.name: digest(path) for path in sorted(INCOMING.iterdir())}
    expected_debs = {item['name'] for item in packages}
    require({name for name in files if name.endswith('.deb')} == expected_debs, 'unexpected deb asset')
    marker = {'schema': 'velnor.apt-verified/v1', 'config': config, 'config_sha256': config_digest(config),
              'suite': suite, 'version': version, 'debian_version': debian, 'commit': commit,
              'ref': ref, 'source_ref': ref, 'packages': packages, 'files': files,
              'source_record_sha256': record_hash, 'manifest_sha256': manifest_hash,
              'workflow_run_id': os.environ.get('GITHUB_RUN_ID', ''),
              'workflow_run_attempt': os.environ.get('GITHUB_RUN_ATTEMPT', ''),
              'consumer_source_sha': consumer_sha}
    write_json(INCOMING / '.apt-verified.json', marker)
    return marker
