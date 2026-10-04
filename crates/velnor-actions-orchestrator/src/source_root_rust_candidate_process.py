"""Bounded candidate children; the candidate owns one process-group capsule."""
import math
import os
import selectors
import signal
import subprocess
import time

from source_intent_cold_common import ColdSourceIntent


_MAX_OUTPUT = 64 * 1024 * 1024
_MAX_TIMEOUT = 900.0


class _ChildFailure(Exception):
    __slots__ = ('reason',)

    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


def _reject(reason):
    raise ColdSourceIntent('root_candidate_child_' + reason)


def _require_capsule():
    pid = os.getpid()
    try:
        if os.getpgrp() != pid or os.getsid(0) != pid:
            _reject('capsule')
    except OSError as error:
        raise ColdSourceIntent('root_candidate_child_capsule') from error


def _require_bounds(limit, timeout):
    if (type(limit) is not int or limit <= 0 or limit > _MAX_OUTPUT
            or type(timeout) not in (int, float) or isinstance(timeout, bool)
            or not math.isfinite(timeout) or timeout <= 0 or timeout > _MAX_TIMEOUT):
        _reject('bounds')


def _kill_capsule(reason, process):
    try:
        os.killpg(os.getpgrp(), signal.SIGKILL)
    except OSError as error:
        if process is not None:
            try:
                process.kill()
                process.wait(timeout=1)
            except (OSError, subprocess.TimeoutExpired) as cleanup_error:
                error = cleanup_error
        raise ColdSourceIntent('root_candidate_child_cleanup') from error
    if process is not None and process.poll() is None:
        try:
            process.kill()
            process.wait(timeout=1)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ColdSourceIntent('root_candidate_child_cleanup') from error
    raise ColdSourceIntent('root_candidate_child_' + reason)


def observe_candidate_child(command, environment, cwd, cap, timeout, *,
                            executable_descriptor=None):
    """Run one direct child inside the current leader's process-group capsule."""
    _require_capsule()
    _require_bounds(cap, timeout)
    execution = {}
    if executable_descriptor is not None:
        if type(executable_descriptor) is not int or executable_descriptor < 0:
            _reject('executable_descriptor')
        execution = {
            'executable': '/proc/self/fd/' + str(executable_descriptor),
            'pass_fds': (executable_descriptor,),
        }
    started = time.monotonic_ns()
    process, selector = None, None
    output = bytearray()
    try:
        if executable_descriptor is not None:
            try:
                os.fstat(executable_descriptor)
            except OSError as error:
                raise _ChildFailure('executable_descriptor') from error
        process = subprocess.Popen(
            command, cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, shell=False,
            close_fds=True, start_new_session=False, **execution)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise _ChildFailure('timeout')
            for key, _events in selector.select(min(0.1, remaining)):
                size = min(65536, cap + 1 - len(output))
                try:
                    data = os.read(key.fileobj.fileno(), size)
                except OSError as error:
                    raise _ChildFailure('read') from error
                if not data:
                    selector.unregister(key.fileobj)
                else:
                    output.extend(data)
                    if len(output) > cap:
                        raise _ChildFailure('output_limit')
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise _ChildFailure('timeout')
        try:
            status = process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            raise _ChildFailure('timeout') from error
        if status != 0:
            raise _ChildFailure('exit')
        return bytes(output), status, time.monotonic_ns() - started
    except _ChildFailure as error:
        _kill_capsule(error.reason, process)
    except BaseException as error:
        _kill_capsule('error', process)
    finally:
        if selector is not None:
            selector.close()
        if process is not None and process.stdout is not None:
            process.stdout.close()
