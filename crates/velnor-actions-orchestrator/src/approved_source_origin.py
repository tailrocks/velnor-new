"""Compiled SOURCE-only origin; runtime acquisition remains unavailable.

Included privately by approved_source_origin.rs with the existing source engine.
No import, descriptor, callback, or environment value supplies the source owner.
"""
import hashlib
import io
import json
import re
import tarfile
import urllib.request
from urllib.parse import urlsplit


_APPROVED_REPOSITORY = 'tailrocks/velnor-new'
_APPROVED_REPOSITORY_ID = 1390620900
_APPROVED_TARGET = 'c57c700459bbe1549fe7eedcb7d8689585c38986'
_APPROVED_FOUNDATION = ('tailrocks/velnor-new/foundation-qualification@'
                        '8758d976a1b25eb387f48aa04ea86f57739b84cf')
_APPROVED_TERMINAL = '3caf51792ecc15f7384d2a0e9f5d6df7155c849a9bc6db3bbb0767bd3d5a27d0'
_APPROVED_REVIEW = '5fbdd15fdbe34fb3564c71aecf7e7e1d216c08eafb5bd299a0fde926cc62ad5c'


class _ApprovedSourceRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, asset):
        super().__init__()
        self.asset, self.redirected = asset, False

    def redirect_request(self, request, pointer, code, message, headers, new_url):
        parts = urlsplit(new_url)
        require(self.asset and not self.redirected and code == 302 and
                parts.scheme == 'https' and parts.hostname == 'release-assets.githubusercontent.com'
                and parts.netloc == parts.hostname and not parts.fragment and
                re.fullmatch(r'/github-production-release-asset/1390620900/[0-9a-f-]+',
                             parts.path), 'approved_source_redirect_origin')
        self.redirected = True
        return urllib.request.Request(new_url, headers={'User-Agent': 'velnor-source-owner/1'},
                                      method='GET')


def _approved_get(endpoint, limit, *, asset=False):
    prefix = 'repos/tailrocks/velnor-new'
    suffix = endpoint[len(prefix):] if type(endpoint) is str and endpoint.startswith(prefix) else None
    allowed = (suffix == '' or type(suffix) is str and re.fullmatch(
        r'/(git/ref/heads/owned-source/(semver-checker|mbx-action)/[0-9a-f]{40}|'
        r'git/ref/tags/owned-source-(semver-checker|mbx-action)-[0-9a-f]{40}|'
        r'git/commits/[0-9a-f]{40}|git/trees/[0-9a-f]{40}\?recursive=1|'
        r'releases/[1-9][0-9]*|releases/assets/[1-9][0-9]*)', suffix))
    require(allowed and
            (not asset or suffix.startswith('/releases/assets/')) and
            type(limit) is int and 0 < limit <= _SNAPSHOT_MAX_ZIP,
            'approved_source_endpoint')
    request = urllib.request.Request('https://api.github.com/' + endpoint, method='GET', headers={
        'Accept': 'application/octet-stream' if asset else 'application/vnd.github+json',
        'User-Agent': 'velnor-source-owner/1', 'X-GitHub-Api-Version': '2022-11-28'})
    # Public origin; no ambient credentials, proxies, or caller transport.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                                        _ApprovedSourceRedirect(asset))
    try:
        with opener.open(request, timeout=40) as response:
            require(response.status == 200, 'approved_source_http_status')
            final = urlsplit(response.geturl())
            require(final.scheme == 'https' and final.netloc in
                    ('api.github.com', 'release-assets.githubusercontent.com') and
                    (asset or response.geturl() == request.full_url),
                    'approved_source_response_origin')
            raw = response.read(limit + 1)
            require(0 < len(raw) <= limit, 'approved_source_response_bound')
            return raw if asset else json.loads(raw, object_pairs_hook=_snapshot_object,
                                                parse_constant=lambda _: require(
                                                    False, 'approved_source_json_number'))
    except (OSError, ValueError, UnicodeError) as error:
        raise ReconcileError('approved_source_transport') from error


def _approved_service(record, release_id, assets):
    prefix = 'repos/' + _APPROVED_REPOSITORY
    repository = _approved_get(prefix, 1024 * 1024)
    require(repository.get('id') == _APPROVED_REPOSITORY_ID and
            repository.get('full_name') == _APPROVED_REPOSITORY and
            repository.get('private') is False, 'approved_source_repository_origin')
    tag = _approved_get(prefix + '/git/ref/tags/' + record.tag, 1024 * 1024)
    require(tag.get('ref') == 'refs/tags/' + record.tag and
            tag.get('object', {}).get('type') == 'commit' and
            tag['object'].get('sha') == record.tag_target, 'approved_source_tag_target')
    branch = _approved_get(prefix + '/git/ref/heads/owned-source/' + record.role.value +
                           '/' + record.source_commit, 1024 * 1024)
    require(branch.get('ref') == record.source_ref and
            branch.get('object', {}).get('type') == 'commit' and
            branch['object'].get('sha') == record.source_commit, 'approved_source_ref')
    commit = _approved_get(prefix + '/git/commits/' + record.source_commit, 1024 * 1024)
    require(commit.get('sha') == record.source_commit and
            commit.get('tree', {}).get('sha') == record.source_tree, 'approved_source_commit')
    tree = _approved_get(prefix + '/git/trees/' + record.source_tree + '?recursive=1',
                         8 * 1024 * 1024)
    entries = _source_entries(tree, record.source_tree)
    _source_verify_trees(entries, record.source_tree)
    release = _approved_get(prefix + '/releases/' + str(release_id), 4 * 1024 * 1024)
    require(release.get('id') == release_id and release.get('tag_name') == record.tag and
            release.get('target_commitish') == record.tag_target and
            release.get('immutable') is True and release.get('draft') is False and
            release.get('prerelease') is False and type(release.get('assets')) is list,
            'approved_source_release')
    actual = release['assets']
    require(len(actual) == len(assets) and {entry.get('name') for entry in actual} == set(assets),
            'approved_source_asset_coverage')
    raw_assets = {}
    for entry in actual:
        name = entry['name']
        asset_id, digest, size = assets[name]
        require(entry.get('id') == asset_id and entry.get('size') == size and
                entry.get('digest') == 'sha256:' + digest and entry.get('state') == 'uploaded'
                and entry.get('url') == 'https://api.github.com/' + prefix +
                '/releases/assets/' + str(asset_id), 'approved_source_asset_origin')
        raw = _approved_get(prefix + '/releases/assets/' + str(asset_id), size, asset=True)
        require(len(raw) == size and hashlib.sha256(raw).hexdigest() == digest,
                'approved_source_asset_bytes')
        raw_assets[name] = raw
    return entries, raw_assets


def _approved_archive(record, entries, raw):
    blobs, found = {}, set()
    prefix = 'source/' if record.archive_kind.value == 'SourcePrefixFilesV1' else ''
    try:
        with tarfile.open(fileobj=io.BytesIO(raw), mode='r:') as archive:
            for member in archive:
                require(member.name.startswith(prefix), 'approved_source_archive_prefix')
                path = member.name[len(prefix):]
                _source_path(path)
                require(path in entries and path not in found, 'approved_source_archive_coverage')
                entry = entries[path]
                if entry['type'] == 'tree':
                    require(member.isdir() and member.mode == 0o755,
                            'approved_source_archive_directory')
                else:
                    require(member.isfile() and member.size == entry['size'] and
                            member.mode == (0o755 if entry['mode'] == '100755' else 0o644),
                            'approved_source_archive_mode')
                    stream = archive.extractfile(member)
                    require(stream is not None, 'approved_source_archive_blob')
                    with stream:
                        content = stream.read(_SOURCE_MAX_BLOB + 1)
                    require(len(content) == entry['size'] and hashlib.sha1(
                        b'blob ' + str(len(content)).encode() + b'\0' + content).hexdigest() ==
                        entry['sha'], 'approved_source_archive_blob_digest')
                    blobs[entry['sha']] = content
                found.add(path)
    except (tarfile.TarError, OSError, UnicodeError) as error:
        raise ReconcileError('approved_source_archive') from error
    expected = {path for path, entry in entries.items() if entry['type'] == 'blob'}
    require(expected <= found and (prefix or found == set(entries)),
            'approved_source_archive_complete')
    snapshot = _SourceSnapshot(_SNAPSHOT_SEAL, _APPROVED_REPOSITORY, record.source_commit,
                               record.source_tree, entries, blobs)
    _materialize_preflight(snapshot)
    return snapshot


def _approved_publication(record, raw_assets):
    publication = json.loads(raw_assets['source-publication.json'], object_pairs_hook=_snapshot_object)
    receipt = json.loads(raw_assets['source-receipt.json'], object_pairs_hook=_snapshot_object)
    require(publication.get('status') == 'SOURCE_ONLY' and
            publication.get('repository') == _APPROVED_REPOSITORY and
            publication.get('tool') == record.role.value and
            publication.get('source_ref') == record.source_ref and
            publication.get('source_commit') == record.source_commit and
            publication.get('source_tree') == record.source_tree and
            publication.get('tag') == record.tag and publication.get('tag_target') == record.tag_target
            and publication.get('behavioral_qualification') is None and
            publication.get('signed_build_provenance') is None and
            publication.get('source_receipt') == receipt, 'approved_source_publication')
    expected_assets = {name: {'sha256': hashlib.sha256(raw).hexdigest(), 'size': len(raw)}
                       for name, raw in raw_assets.items() if name != 'source-publication.json'}
    require(publication.get('assets') == expected_assets and
            publication.get('raw_commit', {}).get('sha256') == record.raw_commit_sha256 and
            publication['raw_commit'].get('git_object_sha1') == record.source_commit,
            'approved_source_publication_asset_relationship')
    raw = raw_assets['source.commit']
    require(hashlib.sha256(raw).hexdigest() == record.raw_commit_sha256 and
            hashlib.sha1(b'commit ' + str(len(raw)).encode() + b'\0' + raw).hexdigest() ==
            record.source_commit and raw.startswith(('tree ' + record.source_tree + '\n').encode()),
            'approved_source_raw_commit')
    for name, expected in (('source.tar', record.source_archive_sha256),
                           ('base.patch', record.base_patch_sha256),
                           ('source-receipt.json', record.source_receipt_sha256)):
        require(hashlib.sha256(raw_assets[name]).hexdigest() == expected,
                'approved_source_reviewed_bytes')


class ApprovedSourceSnapshot:
    __slots__ = ('_snapshot', '_binding')

    def __init__(self):
        require(False, 'approved_source_private_factory')

    def __setattr__(self, _name, _value):
        raise AttributeError('approved_source_immutable')

    def __reduce_ex__(self, _protocol):
        require(False, 'approved_source_not_serializable')

    def require_current(self):
        approved_source_snapshot(self, _SOURCE_MAX_BLOB)

    @property
    def entries(self):
        approved_source_snapshot(self, _SOURCE_MAX_BLOB)
        return self._snapshot.entries

    @property
    def repository(self):
        approved_source_snapshot(self, _SOURCE_MAX_BLOB)
        return self._snapshot.repository

    @property
    def source_sha(self):
        approved_source_snapshot(self, _SOURCE_MAX_BLOB)
        return self._snapshot.source_sha

    @property
    def tree_sha(self):
        approved_source_snapshot(self, _SOURCE_MAX_BLOB)
        return self._snapshot.tree_sha

def _approved_semver_assets(record):
    return {
        'base.patch': (607255512, record.base_patch_sha256, 42058),
        'source-publication.json': (607255564, '6a6278be9b918a5ff4d95cda870ab695fc906cf6a2c0e0b71c46466df07a8d42', 1218993),
        'source-receipt.json': (607255619, record.source_receipt_sha256, 1217089),
        'source.commit': (607255653, record.raw_commit_sha256, 351),
        'source.tar': (607255705, record.source_archive_sha256, record.source_archive_size)}


def _approved_mbx_assets(record):
    return {
        'base.patch': (607259529, record.base_patch_sha256, 2732799),
        'source-publication.json': (607259566, '34f2875abac94b15a7af6bc2fc04106f84481d0d448bc350ed5176ca08279ed0', 2627),
        'source-receipt.json': (607259598, record.source_receipt_sha256, 1102),
        'source.commit': (607259628, record.raw_commit_sha256, 361),
        'source.tar': (607259676, record.source_archive_sha256, record.source_archive_size)}


def _approved_receipt_content(record, raw_assets, snapshot, entries, capsule):
    receipt = json.loads(raw_assets['source-receipt.json'], object_pairs_hook=_snapshot_object)
    require(receipt.get('source_commit') == record.source_commit and
            receipt.get('source_tree') == record.source_tree and
            receipt.get('tool') == record.role.value and
            receipt.get('status') == 'STAGED_SOURCE_ONLY' and
            receipt.get('publication') is None and
            receipt.get('behavioral_qualification') is None and
            receipt.get('signed_build_provenance') is None, 'approved_source_receipt')
    if record.role.value == 'semver-checker':
        files = {path: (entry['mode'].encode(), entry['sha'])
                 for path, entry in entries.items() if entry['type'] == 'blob'}
        payloads = capsule(receipt, raw_assets['source-receipt.json'], raw_assets, record, files)
        require(payloads == {path: snapshot.blobs[entry['sha']] for path, entry in
                             entries.items() if entry['type'] == 'blob'},
                'approved_source_capsule_full_manifest')


def _approved_manifest(snapshot):
    manifest = [{**dict(entry), 'path': path,
                 **({'sha256': hashlib.sha256(snapshot.blobs[entry['sha']]).hexdigest()}
                    if entry['type'] == 'blob' else {})}
                for path, entry in sorted(snapshot.entries.items())]
    manifest_sha = hashlib.sha256(json.dumps(manifest, sort_keys=True,
                                separators=(',', ':')).encode()).hexdigest()
    return manifest_sha


def _approved_boundary():
    issued = {}
    receipt_content = _approved_receipt_content
    manifest_content = _approved_manifest
    semver_assets, mbx_assets = _approved_semver_assets, _approved_mbx_assets
    projection = globals().get('_compiled_approved_source_projection')
    baseline = projection() if projection is not None else None
    records = {}
    if baseline is not None:
        for role, commit in (('semver-checker', '583dddce84706786fc54c41a2c768c28a09c65fd'),
                             ('mbx-action', '62ec0713473dffeab46884b7c03906042794e696')):
            record = baseline[3](role, commit, _APPROVED_TARGET)
            records[role] = (record, tuple(getattr(record, field)
                                          for field in record.__dataclass_fields__))

    def owner():
        require(projection is not None and
                globals().get('_compiled_approved_source_projection') is projection and
                projection() == baseline and baseline[:3] ==
                ('velnor-approved-source-owner-v1', 'source-only', _APPROVED_FOUNDATION),
                'approved_source_owner_replaced')
        for role, (record, fields) in records.items():
            require(baseline[3](role, record.source_commit, _APPROVED_TARGET) is record and
                    tuple(getattr(record, field) for field in record.__dataclass_fields__) == fields,
                    'approved_source_reviewed_record_replaced')
        return baseline[3:]

    def acquire(record, release_id, assets):
        reviewed, capsule = owner()
        require(record is reviewed(record.role.value, record.source_commit, _APPROVED_TARGET),
                'approved_source_reviewed_owner')
        entries, raw_assets = _approved_service(record, release_id, assets)
        _approved_publication(record, raw_assets)
        snapshot = _approved_archive(record, entries, raw_assets['source.tar'])
        receipt_content(record, raw_assets, snapshot, entries, capsule)
        cap = object.__new__(ApprovedSourceSnapshot)
        manifest_sha = manifest_content(snapshot)
        binding = (_APPROVED_REPOSITORY_ID, record.source_commit, record.source_tree,
                   record.role.value, release_id, tuple(sorted(assets.items())),
                   _APPROVED_TERMINAL, _APPROVED_REVIEW, record.owner_manifest_sha256,
                   manifest_sha, 'source-only')
        object.__setattr__(cap, '_snapshot', snapshot)
        object.__setattr__(cap, '_binding', binding)
        issued[id(cap)] = (cap, snapshot, binding)
        return cap

    def validate(cap, limit):
        owner()
        record = issued.get(id(cap))
        require(type(cap) is ApprovedSourceSnapshot and record is not None and
                record[0] is cap and record[1] is cap._snapshot and record[2] is cap._binding,
                'approved_source_expired_or_foreign')
        require(type(limit) is int and 0 <= limit <= _SOURCE_MAX_BLOB, 'approved_source_blob_limit')
        _materialize_preflight(cap._snapshot)
        require((cap._snapshot.repository, cap._snapshot.source_sha, cap._snapshot.tree_sha) ==
                (_APPROVED_REPOSITORY, cap._binding[1], cap._binding[2]) and
                manifest_content(cap._snapshot) == cap._binding[-2],
                'approved_source_issued_manifest_changed')

    def revoke(cap):
        record = issued.get(id(cap))
        require(record is not None and record[0] is cap, 'approved_source_expired_or_foreign')
        del issued[id(cap)]

    def published_semver_source_snapshot():
        reviewed, _ = owner()
        record = reviewed('semver-checker', '583dddce84706786fc54c41a2c768c28a09c65fd', _APPROVED_TARGET)
        return acquire(record, 402356059, semver_assets(record))

    def published_mbx_source_snapshot():
        reviewed, _ = owner()
        record = reviewed('mbx-action', '62ec0713473dffeab46884b7c03906042794e696', _APPROVED_TARGET)
        return acquire(record, 402357500, mbx_assets(record))

    return published_semver_source_snapshot, published_mbx_source_snapshot, validate, revoke


(published_semver_source_snapshot, published_mbx_source_snapshot, approved_source_snapshot,
 revoke_approved_source_snapshot) = _approved_boundary()
del _approved_boundary


def load_approved_source_snapshot():
    """Runtime getter stays absent until genuine compiled transport qualification."""
    return None


def approved_source_blob(source, entry, limit):
    approved_source_snapshot(source, limit)
    require(any(value is entry for value in source._snapshot.entries.values()) and
            entry['type'] == 'blob' and entry['size'] <= limit, 'approved_source_blob_entry')
    return _materialize_blob(source._snapshot, entry)


def approved_source_file(source, path, limit=1024 * 1024):
    approved_source_snapshot(source, limit)
    _source_path(path)
    require(path in source._snapshot.entries, 'approved_source_file_missing')
    return approved_source_blob(source, source._snapshot.entries[path], limit)


def _materialize_approved_source_owned(source, destination):
    approved_source_snapshot(source, _SOURCE_MAX_BLOB)
    return _materialize_source_snapshot_owned(source._snapshot, destination)
