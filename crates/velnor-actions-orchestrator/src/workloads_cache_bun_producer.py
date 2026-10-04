"""Official Bun ingestion in a pure anonymous source producer, never a task job.

The shared npm proof functions are compiled immediately above this module.
Native cache format remains entirely owned by the centrally pinned Bun version.
"""
import pathlib
import shutil
import subprocess
import sys


class SourceFailure(ValueError):
    """Closed nonsecret producer failure classes for the terminal report."""


def owned_directory(path):
    path = pathlib.Path(path)
    for ancestor in [path] + list(path.parents):
        if ancestor.is_symlink():
            raise SourceFailure('unsafe_owned_directory')
        if ancestor.exists() and not ancestor.is_dir():
            raise SourceFailure('unsafe_owned_directory')
    path.mkdir(parents=True, exist_ok=True)
    return path


def synthetic_lock(sources):
    dependencies, packages = {}, {}
    for index, source in enumerate(sources):
        alias = 'velnor-public-source-%d' % index
        selector = source['name'] + '@' + source['version']
        dependencies[alias] = 'npm:' + selector
        packages[alias] = [selector, '', {}, source['integrity']]
    manifest = {'name': 'velnor-public-source-owner', 'dependencies': dependencies}
    lock = {'lockfileVersion': 1, 'configVersion': 1,
            'workspaces': {'': manifest}, 'packages': packages}
    return manifest, lock


def prepare_native(sources, runner, executable, expected_version, transport=open_public):
    if not isinstance(sources, list) or not sources or len(sources) > MAX_PACKAGES:
        raise SourceFailure('source_descriptor_invalid')
    budget = Budget()
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as workers:
        results = list(workers.map(lambda source: qualify_verified(source, budget, transport), sources))
    if any(result != source['integrity'] for source, result in zip(sources, results)):
        raise SourceFailure('public_authority_unavailable')
    executable = pathlib.Path(executable)
    if not executable.is_absolute() or executable.is_symlink() or not executable.is_file():
        raise SourceFailure('native_tool_unqualified')
    runner = pathlib.Path(runner)
    if not runner.is_absolute():
        raise SourceFailure('unsafe_owned_directory')
    workspace = owned_directory(runner / 'velnor/bun-source/package')
    home = owned_directory(runner / 'velnor/bun-source/anonymous-home')
    store = owned_directory(runner / 'velnor/native/bun/install/cache')
    native_temp = owned_directory(runner / 'velnor/bun-source/native-tmp')
    if any(workspace.iterdir()) or any(home.iterdir()) or any(native_temp.iterdir()):
        raise SourceFailure('source_configuration_not_pristine')
    for ancestor in [workspace] + list(workspace.parents):
        for name in ['.npmrc', 'bunfig.toml', '.bunfig.toml']:
            config = ancestor / name
            if config.exists() or config.is_symlink():
                raise SourceFailure('source_configuration_not_pristine')
    manifest, lock = synthetic_lock(sources)
    (workspace / 'package.json').write_text(json.dumps(manifest), encoding='utf-8')
    (workspace / 'bun.lock').write_text(json.dumps(lock), encoding='utf-8')
    environment = {'PATH': '/usr/bin:/bin', 'HOME': str(home),
                   'TMPDIR': str(native_temp), 'TEMP': str(native_temp), 'TMP': str(native_temp),
                   'BUN_INSTALL_CACHE_DIR': str(store)}
    version = subprocess.run([str(executable), '--version'], env=environment,
                             cwd=home, check=True, capture_output=True, text=True)
    if version.stdout.strip() != expected_version:
        raise SourceFailure('native_tool_unqualified')
    # Qualified Bun versions do not recheck SRI on extracted cache hits. Its supported
    # cache removal forces every source through native SRI-checked extraction.
    subprocess.run([str(executable), 'pm', 'cache', 'rm'], env=environment,
                   cwd=workspace, check=True, timeout=120)
    subprocess.run([str(executable), 'install', '--frozen-lockfile',
                    '--ignore-scripts', '--backend', 'copyfile'],
                   env=environment, cwd=workspace, check=True, timeout=120)
    # Only Bun's documented internal top-level temporary directory is discarded.
    # Package members named .git/.tmp remain source bytes owned by Bun.
    temporary = store / '.tmp'
    if temporary.is_symlink():
        raise SourceFailure('unsafe_native_temporary_directory')
    if temporary.exists():
        if not temporary.is_dir():
            raise SourceFailure('unsafe_native_temporary_directory')
        shutil.rmtree(temporary)
    return len(sources)


def bun_source_main():
    error, count = '', 0
    try:
        count = prepare_native([json.loads(source) for source in sys.argv[3:]],
                               os.environ['RUNNER_TEMP'], sys.argv[1], sys.argv[2])
    except PublicSourceDenied:
        error = 'private_or_auth_required'
    except PublicSourceUnavailable:
        error = 'public_authority_unavailable'
    except UnsupportedRegistry:
        error = 'unsupported_registry'
    except PublicSourceIntegrityMismatch:
        error = 'source_integrity_mismatch'
    except SourceFailure as failure:
        error = str(failure)
    except subprocess.TimeoutExpired:
        error = 'native_install_timeout'
    except subprocess.CalledProcessError:
        error = 'native_install_failed'
    except (KeyError, IndexError, json.JSONDecodeError):
        error = 'source_descriptor_invalid'
    except OSError:
        error = 'source_io_error'
    disposition = {'private_or_auth_required': 'PRIVATE_OR_AUTH_REQUIRED',
                   'public_authority_unavailable': 'PUBLIC_AUTHORITY_UNAVAILABLE',
                   'unsupported_registry': 'UNSUPPORTED_REGISTRY'}.get(
                       error, 'SOURCE_VERIFICATION_FAILED' if error else 'NONE')
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
        output.write('verified=%s\nerror=%s\npublic-packages=%d\nsource-error=%s\n'
                     % ('false' if error else 'true', disposition, count, error))
    if error and disposition not in {'PRIVATE_OR_AUTH_REQUIRED',
                                     'PUBLIC_AUTHORITY_UNAVAILABLE', 'UNSUPPORTED_REGISTRY'}:
        raise SystemExit(1)


if __name__ == '__main__':
    bun_source_main()
