"""Verify the sole owner's captured original SDK; never capture or mint authority.

Only the initial trusted Node issuer compiles this seed into its owned capsule.
There is no file/JSON/env loader and no independent hash or platform registry.
"""
import hashlib
import os
import stat


def _fail():
    raise ValueError('foundation_original_changed')


def _absolute(value):
    if not isinstance(value, str) or not value.startswith('/') or os.path.normpath(value) != value:
        _fail()
    return value


def _identity(info, expected):
    if (stat.S_IMODE(info.st_mode), info.st_uid, info.st_gid) != (
            expected['mode'], expected['uid'], expected['gid']):
        _fail()


def _signature(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def _file_hash(filename, expected=None, max_bytes=32 * 1024 ** 3):
    before = os.lstat(filename)
    descriptor = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        opened = os.fstat(descriptor)
        if (not stat.S_ISREG(opened.st_mode) or _signature(opened) != _signature(before)
                or opened.st_size > max_bytes):
            _fail()
        if expected is not None:
            _identity(opened, expected)
            if opened.st_size != expected['size']:
                _fail()
        digest = hashlib.sha256()
        consumed = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            consumed += len(chunk)
            if consumed > opened.st_size:
                _fail()
            digest.update(chunk)
        if (consumed != opened.st_size or _signature(opened) != _signature(os.fstat(descriptor))
                or _signature(opened) != _signature(os.lstat(filename))):
            _fail()
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def _entry(expected):
    filename = _absolute(expected['path'])
    kind = expected['kind']
    if kind in ('absent', 'ancestor-absent'):
        if os.path.lexists(filename):
            _fail()
        return
    info = os.lstat(filename)
    _identity(info, expected)
    if kind in ('symlink', 'ancestor-symlink'):
        if not stat.S_ISLNK(info.st_mode) or os.readlink(filename) != expected['target']:
            _fail()
    elif kind in ('directory', 'ancestor-directory'):
        if not stat.S_ISDIR(info.st_mode):
            _fail()
        if kind == 'directory':
            with os.scandir(filename) as iterator:
                children = []
                for child in iterator:
                    if len(children) >= 100000:
                        _fail()
                    children.append(child.name)
            children.sort(key=lambda value: value.encode('utf-16-be', 'surrogatepass'))
            if children != expected['children'] or _signature(info) != _signature(os.lstat(filename)):
                _fail()
    elif kind == 'file':
        if not stat.S_ISREG(info.st_mode) or _file_hash(filename, expected) != expected['sha256']:
            _fail()
    else:
        _fail()


def _namespace(seed):
    root = _absolute(seed['control_root'])
    expected = seed['namespace']
    current = root
    for item in expected:
        info = os.lstat(current)
        if (item['path'] != current or not stat.S_ISDIR(info.st_mode)
                or info.st_dev != int(item['device']) or info.st_ino != int(item['inode'])):
            _fail()
        _identity(info, item)
        current = os.path.dirname(current)
    if not expected or expected[-1]['path'] != '/':
        _fail()
    for payload in seed['payload_roots']:
        payload = _absolute(payload)
        for compared in (payload, os.path.realpath(payload)):
            if os.path.commonpath((root, compared)) in (root, compared):
                _fail()
    manifest = root + '/foundation-observation.json'
    info = os.lstat(manifest)
    if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077:
        _fail()
    if _file_hash(manifest, max_bytes=64 * 1024 ** 2) != seed['observation_sha256']:
        _fail()


def require_current(seed):
    """Internal verification of an issuer-held literal, not a profile constructor."""
    if not isinstance(seed, dict) or seed['ca_file'] != '/etc/ssl/certs/ca-certificates.crt':
        _fail()
    _namespace(seed)
    records = seed['record']['inventory']
    if not isinstance(records, list) or len(records) > 100000:
        _fail()
    paths = set()
    for expected in records:
        if expected['path'] in paths:
            _fail()
        paths.add(expected['path'])
        _entry(expected)
    if seed['ca_file'] not in paths or seed['record']['executable'] not in paths:
        _fail()
    source_root = _absolute(seed['source_root'])
    for expected in seed['record']['sources']:
        name = expected['path']
        if not isinstance(name, str) or name.startswith('/') or '..' in name.split('/'):
            _fail()
        filename = source_root + '/' + name
        if _file_hash(filename, max_bytes=64 * 1024 ** 2) != expected['sha256']:
            _fail()
    _namespace(seed)
