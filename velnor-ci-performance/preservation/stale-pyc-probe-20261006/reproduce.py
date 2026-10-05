import hashlib
import importlib.util
import json
import marshal
import os
import pathlib
import platform
import struct
import subprocess
import sys
import tarfile
import time

ROOT = pathlib.Path('/private/tmp/velnor-stale-pyc-probe.Izi5SV')
EXPECTED = {
    'scripts/generator-release/extract-candidate.py': '0dd4b0d9ee5bfce20f6d8898639305b92daf6ed0',
    'scripts/owned_archive_preflight.py': '82345d7dc9d18b3689f913687aa537b3ba45cc0f',
}
GIT_COMMIT = 'd3ed26f1a47aa67c8296d00607472f5c63116365'
NAMES = ('runner', 'checksum.txt', 'provenance.json')
MARKER = 'SYNTHETIC_PYC_MODULE_EXECUTED'
RAISE = 'SYNTHETIC_PYC_PREFLIGHT_RAISED'
FIXED_MTIME_NS = 1_700_000_000_250_000_000


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def make_archive(path):
    contents = (b'benign runner payload\n', b'0' * 64 + b'\n', b'{}\n')
    with tarfile.open(path, 'w:', format=tarfile.USTAR_FORMAT) as archive:
        for index, (name, body) in enumerate(zip(NAMES, contents)):
            member = tarfile.TarInfo(name)
            member.size = len(body)
            member.mode = 0o755 if index == 0 else 0o644
            member.type = tarfile.REGTYPE
            member.mtime = 1_700_000_000
            archive.addfile(member, __import__('io').BytesIO(body))


def make_cache(fixture):
    module_path = fixture / 'scripts' / 'owned_archive_preflight.py'
    os.utime(module_path, ns=(FIXED_MTIME_NS, FIXED_MTIME_NS))
    metadata = module_path.stat()
    cache_dir = module_path.parent / '__pycache__'
    cache_dir.mkdir(mode=0o700)
    cache_path = cache_dir / ('owned_archive_preflight.' + sys.implementation.cache_tag + '.pyc')
    stub = (
        "print(" + repr(MARKER) + ", flush=True)\n"
        "def preflight_archive(data, mode):\n"
        "    raise RuntimeError(" + repr(RAISE) + ")\n"
    )
    code = compile(stub, str(module_path), 'exec')
    header = importlib.util.MAGIC_NUMBER + struct.pack(
        '<III', 0, int(metadata.st_mtime) & 0xffffffff, metadata.st_size & 0xffffffff)
    cache_path.write_bytes(header + marshal.dumps(code))
    fields = struct.unpack('<III', cache_path.read_bytes()[4:16])
    assert cache_path.read_bytes()[:4] == importlib.util.MAGIC_NUMBER
    assert fields == (0, int(metadata.st_mtime) & 0xffffffff, metadata.st_size & 0xffffffff)
    return cache_path, metadata, fields


def run_case(name, install_cache):
    fixture = ROOT / name
    module_path = fixture / 'scripts' / 'owned_archive_preflight.py'
    script_path = fixture / 'scripts' / 'generator-release' / 'extract-candidate.py'
    destination = fixture / 'destination'
    archive_path = fixture / 'candidate.tar'
    pycache = module_path.parent / '__pycache__'
    if pycache.exists():
        import shutil
        shutil.rmtree(pycache)
    make_archive(archive_path)
    os.utime(module_path, ns=(FIXED_MTIME_NS, FIXED_MTIME_NS))
    cache_path = None
    cache_fields = None
    if install_cache:
        cache_path, metadata, cache_fields = make_cache(fixture)
    else:
        pycache = module_path.parent / '__pycache__'
        if pycache.exists():
            raise AssertionError('control unexpectedly starts with __pycache__')
        metadata = module_path.stat()
    command = [sys.executable, str(script_path), str(archive_path), str(destination), *NAMES]
    clean_env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin')}
    started = time.monotonic()
    try:
        result = subprocess.run(command, cwd=str(fixture), env=clean_env,
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, timeout=8, check=False)
        duration = time.monotonic() - started
        outcome = {'returncode': result.returncode, 'stdout': result.stdout,
                   'stderr': result.stderr, 'duration_seconds': round(duration, 6),
                   'command': command, 'cwd': str(fixture)}
    except subprocess.TimeoutExpired as error:
        outcome = {'timeout': True, 'timeout_seconds': 8,
                   'stdout': error.stdout, 'stderr': error.stderr,
                   'command': command, 'cwd': str(fixture)}
    outputs = sorted(path.name for path in destination.iterdir())
    return {
        'case': name,
        'cache_installed_before_run': install_cache,
        'cache_path': str(cache_path) if cache_path else None,
        'cache_header_flags_mtime_size': cache_fields,
        'source_path': str(module_path),
        'source_sha256': sha256(module_path),
        'source_git_blob': EXPECTED['scripts/owned_archive_preflight.py'],
        'source_stat_mtime_ns': metadata.st_mtime_ns,
        'source_size': metadata.st_size,
        'archive_size': archive_path.stat().st_size,
        'archive_sha256': sha256(archive_path),
        'outcome': outcome,
        'destination_entries_after_run': outputs,
    }


identity = {
    'probe': 'benign stale timestamp/size pyc import-cache reproduction',
    'commit': GIT_COMMIT,
    'python_executable': sys.executable,
    'python_version': sys.version,
    'python_cache_tag': sys.implementation.cache_tag,
    'python_pyc_magic_hex': importlib.util.MAGIC_NUMBER.hex(),
    'platform': platform.platform(),
    'machine': platform.machine(),
    'cwd': os.getcwd(),
    'bounded_child_timeout_seconds': 8,
    'admitted_blobs': {},
}
for relative, expected_blob in EXPECTED.items():
    for case in ('with-pyc', 'control'):
        copied = ROOT / case / relative
        actual_hash = sha256(copied)
        if actual_hash != sha256(ROOT / 'with-pyc' / relative):
            raise AssertionError('fixture copies differ for ' + relative)
        identity['admitted_blobs'].setdefault(relative, {})[case] = {
            'git_blob': expected_blob,
            'sha256': actual_hash,
            'size': copied.stat().st_size,
        }

print('IDENTITY ' + json.dumps(identity, sort_keys=True))
print('WITH_PYC ' + json.dumps(run_case('with-pyc', True), sort_keys=True))
print('NO_PYC_CONTROL ' + json.dumps(run_case('control', False), sort_keys=True))
