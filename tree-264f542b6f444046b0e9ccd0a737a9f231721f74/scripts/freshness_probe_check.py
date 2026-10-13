"""Run the optional bounded upstream lookup over inventoried pins."""

import json
import re

from freshness_context import norm_version
from freshness_probe import fetch_text


def github_tag(payload):
    if isinstance(payload, dict) and payload.get("tag_name"):
        return payload["tag_name"]
    if isinstance(payload, list):
        for release in payload:
            if not isinstance(release, dict) or release.get("draft") \
                    or release.get("prerelease"):
                continue
            tag = release.get("tag_name") or release.get("name")
            if tag:
                return tag
    return None


def sniff_latest(source, body):
    try:
        payload = json.loads(body)
    except ValueError:
        payload = None
    if payload is not None:
        parsed = _json_release(source, payload)
        if parsed is not None:
            return parsed
    return _html_release(body)


def _json_release(source, payload):
    if "crates.io/api/v1/crates/" in source and isinstance(payload, dict):
        crate = payload.get("crate") or {}
        return crate.get("max_version")
    info = payload.get("info") if isinstance(payload, dict) else None
    if isinstance(info, dict):
        version = info.get("version")
        if isinstance(version, str) and re.fullmatch(r"\d+\.\d+\.\d+", version):
            return version
    tag = github_tag(payload)
    if tag:
        return tag
    crate = (payload.get("crate") or {}) if isinstance(payload, dict) else {}
    return crate.get("max_version")


def _html_release(body):
    python = re.search(r">Download Python (\d+\.\d+\.\d+)<", body)
    if python:
        return python.group(1)
    match = re.search(r"\[pkg\.rust\]\s*\nversion\s*=\s*\""
                      r"(\d+\.\d+\.\d+)", body)
    if not match:
        match = re.search(r'version\s*=\s*"(\d+\.\d+\.\d+)', body)
    return match.group(1) if match else None


def _probe_pin(ctx, subject, source, pinned, stamp):
    try:
        latest = sniff_latest(source, fetch_text(source))
    except Exception as err:  # noqa: BLE001 - probe maps all to failed
        ctx.fail_row("upstream-probe", subject,
                     f"lookup_failed ({err}); source {source}, "
                     f"checked {stamp}")
        return
    if latest is None:
        ctx.fail_row("upstream-probe", subject,
                     f"lookup_failed: no stable release parsed; source "
                     f"{source}, checked {stamp}")
    elif norm_version(latest) != norm_version(pinned):
        ctx.fail_row("upstream-probe", subject,
                     f"stale pin: pinned={pinned!r} latest={latest!r}; "
                     f"source {source}, checked {stamp}")
    else:
        ctx.pass_row("upstream-probe", subject,
                     f"pinned==latest {latest}; source {source}, "
                     f"checked {stamp}")


def check_upstream_probe(ctx):
    stamp = ctx.now.strftime("%Y-%m-%dT%H:%M:%SZ")
    for tool in ctx.tools:
        _probe_pin(ctx, tool.get("name"), tool.get("source", ""),
                   tool.get("pinned"), stamp)
    for key, action in sorted(ctx.action_pinned.items()):
        _probe_pin(ctx, key, action.get("source", ""),
                   action.get("pinned_version"), stamp)
    ctx.info_row("upstream-probe", "runner",
                 "latest image family is platform-qualification evidence, "
                 "not an API probe")
