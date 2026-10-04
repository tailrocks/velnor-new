//! Fixed loader and entrypoints, executed only inside a verified source helper.

pub(super) const LOADER: &str = r#"
_ORDER = ('cache_receipt_common', 'opaque_inventory_metadata', 'metadata_container',
          'source_archive_inventory_common', 'source_archive_inventory_fs',
          'source_archive_inventory_leaf', 'source_archive_inventory_walk',
          'source_archive_inventory', 'cache_receipt_manifest')
if set(_SOURCES) != set(_ORDER):
    raise RuntimeError('receipt_source_registry')
for _name in _ORDER:
    _module = types.ModuleType(_name)
    _module.__file__ = '<compiled-source:' + _name + '>'
    sys.modules[_name] = _module
for _name in _ORDER:
    exec(compile(_SOURCES[_name], '<compiled-source:' + _name + '>', 'exec'),
         sys.modules[_name].__dict__)
del _SOURCES
"#;

pub(super) const ENTRYPOINT: &str = r#"
import base64, hashlib, os, re, secrets, stat
from cache_receipt_common import ColdReceipt, strict_json, secure_read
from cache_receipt_manifest import canonical

def _directory(path):
    if not path.startswith('/') or any(part in ('', '.', '..') for part in path.split('/')[1:]):
        raise ColdReceipt('receipt_directory')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.split('/')[1:]:
            try:
                os.mkdir(part, 0o700, dir_fd=descriptor)
            except FileExistsError:
                pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ColdReceipt('receipt_directory_owner')
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise

def _write(name, data):
    directory = _directory(os.environ['VELNOR_CACHE_RECEIPT_ROOT'])
    temporary = '.receipt-' + secrets.token_hex(16)
    descriptor = None
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=directory)
        remaining = memoryview(data)
        while remaining:
            written = os.write(descriptor, remaining)
            if written <= 0:
                raise ColdReceipt('receipt_write')
            remaining = remaining[written:]
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.rename(temporary, name, src_dir_fd=directory, dst_dir_fd=directory)
        os.fsync(directory)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            os.unlink(temporary, dir_fd=directory)
        except FileNotFoundError:
            pass
        os.close(directory)

def _evidence_complete():
    directory = _directory(os.environ['VELNOR_CACHE_RECEIPT_ROOT'])
    try:
        names = {'manifest.json', 'predicate.json', 'bundle.sigstore.json'}
        if set(os.listdir(directory)) != names:
            raise ColdReceipt('receipt_evidence_files')
        for name in sorted(names):
            descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                                 dir_fd=directory)
            try:
                info = os.fstat(descriptor)
                named = os.stat(name, dir_fd=directory, follow_symlinks=False)
                if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                        or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600
                        or (info.st_dev, info.st_ino) != (named.st_dev, named.st_ino)):
                    raise ColdReceipt('receipt_evidence_file')
            finally:
                os.close(descriptor)
        if set(os.listdir(directory)) != names:
            raise ColdReceipt('receipt_evidence_files')
    finally:
        os.close(directory)

def _number(name):
    value = os.environ.get(name, '')
    if re.fullmatch(r'[1-9][0-9]{0,19}', value) is None or int(value) >= 2**64:
        raise ColdReceipt('receipt_runtime_identity')
    return int(value)

def _manifest():
    repository = os.environ.get('VELNOR_CACHE_SOURCE_REPOSITORY', '')
    source = os.environ.get('VELNOR_CACHE_SOURCE_SHA', '')
    key = os.environ.get('VELNOR_CACHE_RECEIPT_KEY', '')
    if (re.fullmatch(r'[A-Za-z0-9_-][A-Za-z0-9_.-]*/[A-Za-z0-9_-][A-Za-z0-9_.-]*', repository) is None
            or re.fullmatch(r'[a-f0-9]{40}', source) is None
            or re.fullmatch(r'[A-Za-z0-9_.-]{1,2048}', key) is None):
        raise ColdReceipt('receipt_runtime_identity')
    # The producer and consumer use the same exact-root traversal.
    import cache_receipt_manifest as inventory
    if not hasattr(inventory, 'inventory_exact_roots'):
        raise ColdReceipt('payload_root_admission_unavailable')
    data = inventory.inventory_exact_roots(os.environ['VELNOR_CACHE_PAYLOAD_ROOT'],
                                    tuple(_CONFIG['allowed_roots']), tuple(_CONFIG['optional_roots']))
    predicate = {name: _CONFIG[name] for name in
                 ('role', 'descriptor_sha256', 'helper_sha256', 'catalog_sha256', 'policy_sha256')}
    predicate.update(schema=1, cache_key=key, manifest_sha256=hashlib.sha256(data).hexdigest(),
        repository_id=str(_number('VELNOR_CACHE_SOURCE_REPOSITORY_ID')), source_sha=source,
        run_id=_number('VELNOR_CACHE_RUN_ID'), run_attempt=_number('VELNOR_CACHE_RUN_ATTEMPT'))
    _write('manifest.json', data)
    _write('predicate.json', canonical(predicate))

def _public_base64(value, limit):
    if not isinstance(value, str):
        raise ColdReceipt('public_bundle_encoding')
    try:
        data = base64.b64decode(value, validate=True)
    except ValueError as error:
        raise ColdReceipt('public_bundle_encoding') from error
    if not 1 <= len(data) <= limit:
        raise ColdReceipt('public_bundle_encoding')
    return data

def _public_tlog(entry):
    allowed = {'logIndex', 'logId', 'kindVersion', 'integratedTime', 'canonicalizedBody',
               'inclusionPromise', 'inclusionProof'}
    if (not isinstance(entry, dict) or set(entry) - allowed
            or not {'logId', 'kindVersion', 'canonicalizedBody'} <= set(entry)
            or entry['kindVersion'] != {'kind': 'dsse', 'version': '0.0.1'}
            or not isinstance(entry['logId'], dict) or set(entry['logId']) != {'keyId'}):
        raise ColdReceipt('public_bundle_log_shape')
    _public_base64(entry['logId']['keyId'], 128)
    _public_base64(entry['canonicalizedBody'], 8 * 1024 * 1024)
    for name in ('logIndex', 'integratedTime'):
        if name in entry and (not isinstance(entry[name], str)
                or re.fullmatch(r'[0-9]{1,20}', entry[name]) is None):
            raise ColdReceipt('public_bundle_log_shape')
    if not {'inclusionPromise', 'inclusionProof'} & set(entry):
        raise ColdReceipt('public_bundle_log_evidence')
    if 'inclusionPromise' in entry:
        promise = entry['inclusionPromise']
        if not isinstance(promise, dict) or set(promise) != {'signedEntryTimestamp'}:
            raise ColdReceipt('public_bundle_log_shape')
        _public_base64(promise['signedEntryTimestamp'], 65536)
    if 'inclusionProof' in entry:
        proof = entry['inclusionProof']
        if (not isinstance(proof, dict) or set(proof) - {'logIndex', 'rootHash', 'treeSize', 'hashes', 'checkpoint'}
                or not {'rootHash', 'checkpoint'} <= set(proof)
                or not isinstance(proof['checkpoint'], dict)
                or set(proof['checkpoint']) != {'envelope'}
                or not isinstance(proof['checkpoint']['envelope'], str)
                or not 1 <= len(proof['checkpoint']['envelope']) <= 65536):
            raise ColdReceipt('public_bundle_log_shape')
        _public_base64(proof['rootHash'], 128)
        for name in ('logIndex', 'treeSize'):
            if name in proof and (not isinstance(proof[name], str)
                    or re.fullmatch(r'[0-9]{1,20}', proof[name]) is None):
                raise ColdReceipt('public_bundle_log_shape')
        if 'hashes' in proof:
            if not isinstance(proof['hashes'], list) or len(proof['hashes']) > 128:
                raise ColdReceipt('public_bundle_log_shape')
            for digest in proof['hashes']:
                _public_base64(digest, 128)

def _bundle():
    root = os.environ['VELNOR_CACHE_RECEIPT_ROOT']
    data = secure_read(os.environ['VELNOR_CACHE_RECEIPT_BUNDLE_PATH'])
    # JWT syntax is forbidden even inside otherwise public envelope metadata.
    if re.search(rb'eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}', data):
        raise ColdReceipt('private_credential_in_bundle')
    value = strict_json(data)
    if (not isinstance(value, dict) or set(value) != {'mediaType', 'verificationMaterial', 'dsseEnvelope'}
            or value['mediaType'] != 'application/vnd.dev.sigstore.bundle.v0.3+json'):
        raise ColdReceipt('public_bundle_shape')
    envelope = value['dsseEnvelope']
    material = value['verificationMaterial']
    if (not isinstance(envelope, dict) or set(envelope) != {'payload', 'payloadType', 'signatures'}
            or not isinstance(material, dict)
            or set(material) != {'certificate', 'tlogEntries', 'timestampVerificationData'}
            or material['timestampVerificationData'] != {}
            or not isinstance(material['certificate'], dict)
            or set(material['certificate']) != {'rawBytes'}
            or not isinstance(material.get('tlogEntries'), list) or len(material['tlogEntries']) != 1
            or envelope.get('payloadType') != 'application/vnd.in-toto+json'
            or not isinstance(envelope.get('payload'), str)
            or not isinstance(envelope.get('signatures'), list) or len(envelope['signatures']) != 1
            or not isinstance(envelope['signatures'][0], dict)
            or set(envelope['signatures'][0]) != {'sig'}):
        raise ColdReceipt('public_bundle_evidence_missing')
    try:
        _public_base64(material['certificate']['rawBytes'], 65536)
        _public_base64(envelope['signatures'][0]['sig'], 65536)
        _public_tlog(material['tlogEntries'][0])
        statement = strict_json(base64.b64decode(envelope['payload'], validate=True))
    except (ValueError, TypeError) as error:
        raise ColdReceipt('public_bundle_statement') from error
    subject = secure_read(root + '/manifest.json')
    predicate = strict_json(secure_read(root + '/predicate.json'))
    expected = {'_type': 'https://in-toto.io/Statement/v1',
        'subject': [{'name': 'manifest.json', 'digest': {'sha256': hashlib.sha256(subject).hexdigest()}}],
        'predicateType': _CONFIG['predicate_type'], 'predicate': predicate}
    if statement != expected:
        raise ColdReceipt('public_bundle_statement_changed')
    _write('bundle.sigstore.json', data)
    _evidence_complete()

try:
    {'manifest': _manifest, 'bundle': _bundle}[_COMMAND]()
except (ColdReceipt, OSError, KeyError, ValueError, RecursionError):
    # Report a fixed reason only; never include envelope/body/environment bytes.
    sys.stderr.write('cache_receipt_production_rejected\n')
    sys.exit(1)
"#;
