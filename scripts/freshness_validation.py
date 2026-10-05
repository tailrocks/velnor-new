"""Check activated manual validation-tool pins and their source scope."""

import glob
import os
import re


MUTANTS = ".cargo/mutants.toml"


def _read_mutants(ctx):
    try:
        with open(ctx.path(MUTANTS), encoding="utf-8") as handle:
            return handle.read()
    except OSError as err:
        ctx.fail_row("local-pin", MUTANTS, f"unreadable ({err})")
        return None


def _check_mutants_pin(ctx, mutants_text):
    pinned_tools = ctx.policy.get("validation-tools", {})
    if "cargo-mutants" not in pinned_tools:
        ctx.fail_row("local-pin", "validation-tools/cargo-mutants",
                     "policy pin missing")
        return
    want = pinned_tools["cargo-mutants"]
    match = re.search(r'^# pinned: cargo-mutants = "([^"]+)"',
                      mutants_text, re.MULTILINE)
    if not match:
        ctx.fail_row("local-pin", "validation-tools/cargo-mutants",
                     f'{MUTANTS} lacks a `# pinned: cargo-mutants = "x"` line')
    elif match.group(1) != want:
        ctx.fail_row("local-pin", "validation-tools/cargo-mutants",
                     f"mutants pin={match.group(1)!r} policy={want!r}")
    else:
        ctx.pass_row("local-pin", "validation-tools/cargo-mutants", want)


def _scope_globs(ctx, mutants_text):
    in_scope = False
    globs = []
    for line in mutants_text.splitlines():
        stripped = line.strip()
        if stripped.startswith("examine_globs"):
            in_scope = True
            continue
        if in_scope:
            if stripped.startswith("]"):
                break
            globs.extend(re.findall(r'"([^"]+)"', stripped.split("#")[0]))
    if not globs:
        ctx.fail_row("local-pin", f"{MUTANTS} examine_globs", "scope is empty")
    return globs


def _check_scope_matches(ctx, mutants_text):
    for pattern in sorted(_scope_globs(ctx, mutants_text)):
        hits = glob.glob(f"{ctx.root}/{pattern}", recursive=True)
        if not [hit for hit in hits if os.path.isfile(hit)]:
            ctx.fail_row("local-pin", f"{MUTANTS} scope {pattern}",
                         "glob matches no production file")
        else:
            ctx.pass_row("local-pin", f"{MUTANTS} scope {pattern}",
                         f"{len(hits)} match(es)")


def check_validation_tool_pins(ctx):
    mutants_text = _read_mutants(ctx)
    if mutants_text is None or ctx.policy is None:
        return
    _check_mutants_pin(ctx, mutants_text)
    _check_scope_matches(ctx, mutants_text)
