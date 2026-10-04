"""Execute the actual native producer logic with bounded source/native stand-ins."""
import json
import os
import pathlib
import sys
import tempfile

expected_version = sys.argv[3]
scope = {'__name__': 'test_library'}
exec(compile(pathlib.Path(sys.argv[1]).read_text(), 'public.py', 'exec'), scope)
exec(compile(pathlib.Path(sys.argv[2]).read_text(), 'native.py', 'exec'), scope)
source = {'name': 'a', 'version': '1.2.3',
          'resolved': 'https://registry.npmjs.org/a/-/a-1.2.3.tgz',
          'integrity': 'sha512-' + 'A' * 86 + '=='}
manifest, lock = scope['synthetic_lock']([source, {**source, 'version': '2.0.0'}])
assert len(manifest['dependencies']) == len(lock['packages']) == 2
assert not {'scripts', 'trustedDependencies'} & manifest.keys()
with tempfile.TemporaryDirectory() as raw:
    root = pathlib.Path(raw).resolve()
    executable = root / 'bun'
    executable.write_text('''#!/bin/sh
if test "$1" = --version; then echo EXPECTED_VERSION; exit 0; fi
if test "$1" = pm && test "$2" = cache && test "$3" = rm; then rm -rf "$BUN_INSTALL_CACHE_DIR"; exit 0; fi
test "$1" = install && test "$2" = --frozen-lockfile && test "$3" = --ignore-scripts
test "$4" = --backend && test "$5" = copyfile || exit 9
test -z "${NPM_TOKEN:-}" && test -z "${BUN_CONFIG_DEFAULT_REGISTRY:-}" || exit 10
mkdir -p "$BUN_INSTALL_CACHE_DIR/a@1.2.3@@@1/.git"
printf PUBLIC > "$BUN_INSTALL_CACHE_DIR/a@1.2.3@@@1/.git/config"
'''.replace('EXPECTED_VERSION', expected_version))
    executable.chmod(0o755)
    scope['qualify_verified'] = lambda source, budget, transport: None
    try:
        scope['prepare_native']([source], root / 'rejected', executable, expected_version)
        raise AssertionError('private source admitted')
    except ValueError:
        pass
    assert not (root / 'rejected').exists()
    saved_arguments = sys.argv[:]
    sys.argv = ['producer', str(executable), expected_version, json.dumps(source)]
    os.environ['RUNNER_TEMP'] = str(root / 'rejected')
    for kind, disposition in [
            ('PublicSourceDenied', 'PRIVATE_OR_AUTH_REQUIRED'),
            ('PublicSourceUnavailable', 'PUBLIC_AUTHORITY_UNAVAILABLE'),
            ('PublicSourceIntegrityMismatch', 'SOURCE_VERIFICATION_FAILED'),
            ('UnsupportedRegistry', 'UNSUPPORTED_REGISTRY')]:
        def reject(source, budget, transport):
            raise scope[kind]('PRIVATE_CANARY')
        scope['qualify_verified'] = reject
        output = root / kind
        os.environ['GITHUB_OUTPUT'] = str(output)
        try:
            scope['bun_source_main']()
            assert disposition != 'SOURCE_VERIFICATION_FAILED'
        except SystemExit as failure:
            assert failure.code == 1 and disposition == 'SOURCE_VERIFICATION_FAILED'
        contents = output.read_text()
        assert 'verified=false\nerror=' + disposition + '\n' in contents
        assert 'PRIVATE_CANARY' not in contents
    sys.argv = saved_arguments
    scope['qualify_verified'] = lambda source, budget, transport: source['integrity']
    os.environ['NPM_TOKEN'] = 'PRIVATE'
    os.environ['BUN_CONFIG_DEFAULT_REGISTRY'] = 'https://private.example'
    stale = root / 'accepted/velnor/native/bun/install/cache/poisoned-package'
    stale.parent.mkdir(parents=True)
    stale.write_text('PRIVATE_STALE_CACHE_MEMBER')
    assert scope['prepare_native']([source], root / 'accepted', executable, expected_version) == 1
    assert not stale.exists()
    store = root / 'accepted/velnor/native/bun/install/cache'
    assert (store / 'a@1.2.3@@@1/.git/config').read_text() == 'PUBLIC'
    executable.write_text('#!/bin/sh\nif test "$1" = --version; then echo EXPECTED_VERSION; exit 0; fi\nexit 12\n'.replace('EXPECTED_VERSION', expected_version))
    try:
        scope['prepare_native']([source], root / 'failed', executable, expected_version)
        raise AssertionError('native failure admitted')
    except scope['subprocess'].CalledProcessError:
        pass
    executable.write_text('#!/bin/sh\nif test "$1" = --version; then echo not-qualified; exit 0; fi\nexit 0\n')
    try:
        scope['prepare_native']([source], root / 'wrong-tool', executable, expected_version)
        raise AssertionError('unqualified tool accepted')
    except ValueError:
        pass
print('Bun native producer authority/failure/config-isolation fixture passed')
