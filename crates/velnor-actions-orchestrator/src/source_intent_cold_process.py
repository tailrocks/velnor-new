"""Bounded actual child observations for fixed-source cold installation."""
import os
import selectors
import signal
import subprocess
import time

from source_intent_cold_common import ColdSourceIntent


def observe_install_child(command, environment, cwd, limit, timeout, *, executable_descriptor=None):
    started = time.monotonic_ns()
    execution = {}
    if executable_descriptor is not None:
        if type(executable_descriptor) is not int or executable_descriptor < 0:
            raise ColdSourceIntent('cold_sdk_child_executable_descriptor')
        execution = {'executable': '/proc/self/fd/' + str(executable_descriptor),
                     'pass_fds': (executable_descriptor,)}
    process = subprocess.Popen(command, cwd=cwd, env=environment,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        start_new_session=True, **execution)
    selector, output = selectors.DefaultSelector(), bytearray()
    deadline = time.monotonic() + timeout
    try:
        selector.register(process.stdout, selectors.EVENT_READ)
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise ColdSourceIntent('cold_sdk_child_timeout')
            for key, _ in selector.select(0.1):
                data = os.read(key.fileobj.fileno(), min(65536, limit + 1 - len(output)))
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                output.extend(data)
                if len(output) > limit:
                    raise ColdSourceIntent('cold_sdk_child_output_limit')
        try:
            status = process.wait(timeout=max(0.01, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise ColdSourceIntent('cold_sdk_child_timeout') from error
        return bytes(output), status, time.monotonic_ns() - started
    finally:
        # The trusted installer owns a fresh session; end inherited descendants
        # even when its direct child already exited. Repository execution needs
        # the separate qualified namespace/seccomp boundary.
        cleanup_error = None
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError as error:
            cleanup_error = error
            if process.poll() is None:
                try:
                    process.kill()
                except OSError as kill_error:
                    cleanup_error = kill_error
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired as error:
            cleanup_error = error
        finally:
            selector.close()
            process.stdout.close()
        if cleanup_error is not None:
            raise ColdSourceIntent('cold_sdk_child_cleanup_failed') from cleanup_error
