"""Execute admitted compiled bytes through a sealed or read-only descriptor."""
import fcntl
import hashlib
import os
import select
import subprocess
import sys
import threading


def temporary_root(path):
    if not path.startswith('/') or any(part in ('', '.', '..') for part in path.split('/')[1:]):
        raise SystemExit('source_helper_temp_path')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.split('/')[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise SystemExit('source_helper_temp_owner')
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def materialize(source):
    if sys.platform == 'linux':
        descriptor = os.memfd_create('velnor-source-helper', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        try:
            os.fchmod(descriptor, 0o700)
            write_source(descriptor, source)
            seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
            fcntl.fcntl(descriptor, fcntl.F_ADD_SEALS, seals)
            if fcntl.fcntl(descriptor, fcntl.F_GET_SEALS) != seals:
                raise SystemExit('source_helper_seal')
            return descriptor, None
        except BaseException:
            os.close(descriptor)
            raise
    if sys.platform != 'darwin':
        raise SystemExit('source_helper_unsupported_platform')
    descriptor, writer = os.pipe()
    os.set_blocking(writer, False)
    stop = threading.Event()
    failures = []
    feeder = threading.Thread(target=feed_pipe, args=(writer, source, stop, failures), daemon=True)
    feeder.start()
    return descriptor, (feeder, stop, failures)


def feed_pipe(writer, source, stop, failures):
    try:
        remaining = memoryview(source)
        while remaining and not stop.is_set():
            if not select.select([], [writer], [], 0.1)[1]:
                continue
            try:
                written = os.write(writer, remaining)
            except BlockingIOError:
                continue
            if written <= 0:
                raise OSError('source_helper_short_write')
            remaining = remaining[written:]
    except BrokenPipeError:
        pass
    except BaseException as error:
        failures.append(error)
    finally:
        os.close(writer)


def close_source(descriptor, feeder):
    os.close(descriptor)
    if feeder is not None:
        thread, stop, failures = feeder
        stop.set()
        thread.join(timeout=2)
        if thread.is_alive() or failures:
            raise SystemExit('source_helper_pipe_write')


def write_source(descriptor, source):
    remaining = memoryview(source)
    while remaining:
        written = os.write(descriptor, remaining)
        if written <= 0:
            raise SystemExit('source_helper_short_write')
        remaining = remaining[written:]
    os.lseek(descriptor, 0, os.SEEK_SET)


def execution_environment():
    credentials = {'GITHUB_TOKEN', 'GH_TOKEN', 'MISE_GITHUB_TOKEN',
                   'ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN',
                   'ACTIONS_ID_TOKEN_REQUEST_URL', 'GH_HOST', 'GH_CONFIG_DIR',
                   'NODE_AUTH_TOKEN', 'NPM_TOKEN', 'CARGO_REGISTRY_TOKEN'}
    startup = {'BASH_ENV', 'ENV', 'PYTHONPATH', 'PYTHONHOME', 'CDPATH',
               'LD_PRELOAD', 'LD_LIBRARY_PATH', 'LD_AUDIT', 'DYLD_INSERT_LIBRARIES',
               'DYLD_LIBRARY_PATH', 'DYLD_FRAMEWORK_PATH',
               'DYLD_FALLBACK_LIBRARY_PATH', 'DYLD_FALLBACK_FRAMEWORK_PATH'}
    environment = {key: value for key, value in os.environ.items()
            if key not in credentials | startup
            and not key.startswith('VELNOR_COMPILED_HELPER_')
            and not key.endswith('_TOKEN') and not key.startswith('CARGO_REGISTRIES_')}
    environment['RUSTUP_AUTO_INSTALL'] = '0'
    return environment


def execute(source, digest, arguments, prefix):
    encoded = source.encode('utf-8')
    if len(encoded) > 262144 or hashlib.sha256(encoded).hexdigest() != digest:
        raise SystemExit('source_helper_digest')
    root = temporary_root(os.environ['RUNNER_TEMP'])
    environment = execution_environment()
    invocation = [*prefix, *arguments]
    argument_size = sum(len(arg.encode('utf-8')) + 1 for arg in invocation)
    environment_size = sum(len(key.encode()) + len(value.encode()) + 2
                           for key, value in environment.items())
    pointers = 8 * (len(invocation) + len(environment) + 16)
    if (sys.platform == 'linux' and any(len(arg.encode('utf-8')) >= 131072 for arg in invocation)) or argument_size + environment_size + pointers + 8192 >= os.sysconf('SC_ARG_MAX'):
        os.close(root)
        raise SystemExit('source_helper_argument_size')
    try:
        descriptor, feeder = materialize(encoded)
    finally:
        os.close(root)
    try:
        if feeder is None:
            with os.fdopen(os.dup(descriptor), 'rb') as stream:
                if hashlib.sha256(stream.read()).hexdigest() != digest:
                    raise SystemExit('source_helper_materialized_digest')
            os.lseek(descriptor, 0, os.SEEK_SET)
        command = [*prefix, '/bin/bash', '-p', '/dev/fd/' + str(descriptor), *arguments]
        result = subprocess.run(command, env=environment,
                                pass_fds=(descriptor,), check=False)
        return result.returncode if result.returncode >= 0 else 128 - result.returncode
    finally:
        close_source(descriptor, feeder)
