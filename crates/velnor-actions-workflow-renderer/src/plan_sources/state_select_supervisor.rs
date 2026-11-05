// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const SUPERVISOR: &str = r#"import os
import math
import pathlib
import signal
import subprocess
import sys
import time

d = pathlib.Path(os.environ["VELNOR_STATE_SELECT_DIR"])
started = time.monotonic()
active = {}
guardian = """d=$VELNOR_STATE_SELECT_DIR
tag=$VELNOR_STATE_SELECT_TAG
kind=$VELNOR_STATE_SELECT_KIND
prefix=$VELNOR_STATE_SELECT_UNIT_PREFIX
unit=$VELNOR_STATE_SELECT_UNIT
pgid=$$
trap ':' TERM
while [ ! -e "$d/ready-$pgid" ]; do sleep 0.01; done
if [ "$kind" = list ]; then
  gh cache list -R "$GITHUB_REPOSITORY" --key "$VELNOR_STATE_SELECT_PREFIX" --ref "$VELNOR_DEFAULT_REF" --sort created_at --order desc --limit "$VELNOR_STATE_SELECT_LIST_LIMIT" --json key,createdAt,id,sizeInBytes --jq '.[] | [.createdAt, .key, (.id | tostring), (.sizeInBytes | tostring)] | @tsv' > "$d/list" 2>/dev/null
  result=$?
else
  bash -c "$worker" bash "$prefix" "$unit" "$pgid"
  result=$?
fi
status_tmp="$d/status-$tag-$pgid"
if printf '%s\\n' "$result" > "$status_tmp" && mv -f "$status_tmp" "$d/status-$tag"; then :; else rm -f "$status_tmp"; fi
while [ ! -e "$d/release-$pgid" ]; do sleep 0.01; done
exit 0"""

class AggregateExpired(Exception):
    pass

class Cancelled(Exception):
    def __init__(self, signum):
        self.signum = signum

def clear_outputs():
    for output in d.glob("u-*/out"):
        output.write_text("")

def clear_units(units):
    for unit in units:
        (d / f"u-{unit}" / "out").write_text("")

def observed_status(tag):
    try:
        return int((d / f"status-{tag}").read_text().strip())
    except (OSError, ValueError):
        return None

def stop_groups(tags, grace=0.2):
    old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, set(handled_signals))
    try:
        groups = [
            (tag, active[tag][0], (d / f"status-{tag}").exists())
            for tag in tags
            if tag in active
        ]
        for tag, proc, published in groups:
            if published:
                continue
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        if any(not published for _, _, published in groups):
            time.sleep(grace)
        for _, proc, _ in groups:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for tag, _, _ in groups:
            active.pop(tag, None)
        for tag, proc, _ in groups:
            try:
                proc.wait()
            finally:
                (d / f"ready-{proc.pid}").unlink(missing_ok=True)
                (d / f"release-{proc.pid}").unlink(missing_ok=True)
                (d / f"status-{tag}").unlink(missing_ok=True)
                (d / f"status-{tag}-{proc.pid}").unlink(missing_ok=True)
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)

def on_signal(signum, frame):
    raise Cancelled(signum)

handled_signals = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)
original_handlers = {sig: signal.getsignal(sig) for sig in handled_signals}
for sig in handled_signals:
    signal.signal(sig, on_signal)

def reset_child_signals():
    signal.pthread_sigmask(signal.SIG_SETMASK, set())

def launch(tag, kind, prefix="", unit=""):
    old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT, signal.SIGHUP})
    try:
        env = os.environ.copy()
        env.update({
            "VELNOR_STATE_SELECT_TAG": tag,
            "VELNOR_STATE_SELECT_KIND": kind,
            "VELNOR_STATE_SELECT_UNIT_PREFIX": prefix,
            "VELNOR_STATE_SELECT_UNIT": unit,
        })
        proc = subprocess.Popen(
            ["bash", "-c", guardian],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            preexec_fn=reset_child_signals,
        )
        active[tag] = (proc, time.monotonic())
        (d / f"ready-{proc.pid}").touch()
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)
    return proc

def check_deadline():
    if time.monotonic() - started >= deadline:
        raise AggregateExpired()

def await_group(tag):
    while True:
        check_deadline()
        status_path = d / f"status-{tag}"
        if status_path.exists():
            try:
                return int(status_path.read_text().strip())
            except ValueError:
                return 1
        time.sleep(0.01)

def finish(tags):
    return stop_groups(tags)

def main():
    global deadline, unit_timeout, parallelism, list_limit, listing_ok, all_units
    deadline = float(os.environ["VELNOR_STATE_SELECT_DEADLINE"])
    unit_timeout = float(os.environ["VELNOR_STATE_UNIT_TIMEOUT"])
    parallelism = int(os.environ["VELNOR_STATE_PARALLELISM"])
    list_limit = int(os.environ["VELNOR_STATE_SELECT_LIST_LIMIT"])
    if not math.isfinite(deadline) or not math.isfinite(unit_timeout) or not 0 < deadline <= 3600 or not 0 < unit_timeout <= 3600 or not 1 <= parallelism <= 32 or not 0 < list_limit <= 1000:
        return
    launch("list", "list")
    list_status = await_group("list")
    finish(["list"])
    if list_status != 0:
        return
    rows = []
    try:
        for line in (d / "list").read_text().splitlines():
            fields = line.split("\t")
            if len(fields) != 4 or any(not field for field in fields):
                return
            created, key, identifier, size = fields
            time.strptime(created, "%Y-%m-%dT%H:%M:%SZ")
            if not identifier.isascii() or not identifier.isdecimal() or not size.isascii() or not size.isdecimal():
                return
            rows.append((created, key, identifier, size))
    except (OSError, ValueError):
        return
    if len(rows) >= list_limit:
        return
    (d / "sorted").write_text("\n".join("\t".join(row) for row in sorted(rows, reverse=True)) + ("\n" if rows else ""))
    units = [line.split(" ") for line in (d / "units").read_text().splitlines()]
    if any(len(unit) != 6 for unit in units):
        return
    all_units = [unit[1] for unit in units]
    listing_ok = True
    next_unit = 0
    while next_unit < len(units) or active:
        check_deadline()
        while next_unit < len(units) and len(active) < parallelism:
            prefix, unit = units[next_unit][0:2]
            tag = f"u{next_unit}"
            (d / f"u-{unit}" / "out").write_text("")
            launch(tag, "unit", prefix, unit)
            next_unit += 1
        finished = []
        expired_units = []
        now = time.monotonic()
        for tag, (_, spawned) in list(active.items()):
            status = d / f"status-{tag}"
            if tag != "list" and now - spawned >= unit_timeout:
                unit = units[int(tag[1:])][1]
                finished.append((tag, None))
                expired_units.append(unit)
            elif status.exists():
                finished.append((tag, observed_status(tag)))
        if finished:
            finish([tag for tag, _ in finished])
            for tag, status in finished:
                if tag == "list":
                    continue
                unit = units[int(tag[1:])][1]
                if status != 0:
                    clear_units([unit])
            for unit in expired_units:
                clear_units([unit])
        elif active:
            time.sleep(0.01)

def publish_outputs():
    output_path = os.environ.get("GITHUB_OUTPUT")
    if not output_path:
        return
    lines = []
    for record in (d / "units").read_text().splitlines():
        prefix, unit, attested_name, produce_name, compat, snapshot = record.split(" ")
        selected = (d / f"u-{unit}" / "out").read_text().splitlines()
        lines.extend((f"{attested_name}<<VELNOR_STATE_ATTESTED_EOF", *selected, "VELNOR_STATE_ATTESTED_EOF"))
        match_prefix = f"{prefix}{compat}-{snapshot}-" if compat and snapshot else None
        produce = bool(match_prefix and not any(candidate.startswith(match_prefix) for candidate in selected))
        lines.append(f"{produce_name}={str(produce).lower()}")
    with open(output_path, "a", encoding="utf-8") as output:
        output.write("\n".join(lines) + "\n")

listing_ok = False
all_units = []
try:
    main()
except AggregateExpired:
    for sig in handled_signals:
        signal.signal(sig, signal.SIG_IGN)
    unfinished = [all_units[int(tag[1:])] for tag in active if tag.startswith("u")]
    stop_groups(list(active))
    clear_units(unfinished)
    if not listing_ok:
        clear_outputs()
    publish_outputs()
    sys.exit(0)
except Cancelled as cancelled:
    for sig in handled_signals:
        signal.signal(sig, signal.SIG_IGN)
    stop_groups(list(active), 1)
    clear_outputs()
    publish_outputs()
    sys.exit(128 + cancelled.signum)
except BaseException:
    for sig in handled_signals:
        signal.signal(sig, signal.SIG_IGN)
    stop_groups(list(active))
    clear_outputs()
    publish_outputs()
    sys.exit(0)
else:
    publish_outputs()
finally:
    if active:
        stop_groups(list(active))
    for sig, handler in original_handlers.items():
        signal.signal(sig, handler)
"#;
