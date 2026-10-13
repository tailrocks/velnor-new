"""Check effective Cargo dependency identity and lock-graph closure."""

import glob
import os
import tomllib


DEP_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def walk_dep_tables(node, prefix):
    """Collect dependency entries under regular and target-specific tables."""
    found = []
    if not isinstance(node, dict):
        return found
    for section in DEP_SECTIONS:
        table = node.get(section)
        if isinstance(table, dict):
            scope = f"{prefix}{section}" if prefix else section
            found.extend((scope, key, table[key]) for key in sorted(table))
    target = node.get("target")
    if isinstance(target, dict):
        for name in sorted(target):
            found.extend(walk_dep_tables(target[name], f"target.{name}."))
    return found


def lock_identity(entry):
    name = entry.get("name", "")
    version = (entry.get("version") or "").split("+", 1)[0]
    source = entry.get("source") or "local"
    return (name, version, source)


def _read_workspace(ctx):
    try:
        with open(ctx.path("Cargo.toml"), "rb") as handle:
            doc = tomllib.load(handle)
        return (doc.get("workspace") or {}).get("dependencies", {})
    except (OSError, tomllib.TOMLDecodeError) as err:
        ctx.fail_row("lock-staleness", "workspace Cargo.toml",
                     f"unreadable ({err})")
        return None


def _read_lock(ctx):
    try:
        with open(ctx.path("Cargo.lock"), "rb") as handle:
            return tomllib.load(handle).get("package", [])
    except (OSError, tomllib.TOMLDecodeError, AttributeError) as err:
        ctx.fail_row("lock-staleness", "Cargo.lock", f"unreadable ({err})")
        return None


def _declared_dependency(ctx, crate, scope, alias, spec, inherited):
    subject = f"{crate}:{scope}:{alias}"
    if isinstance(spec, str):
        return (subject, alias, spec, None)
    if not isinstance(spec, dict):
        ctx.fail_row("lock-staleness", subject, f"malformed spec {spec!r}")
        return None
    if spec.get("git"):
        ctx.fail_row("lock-staleness", subject,
                     f"git dependency forbidden ({spec.get('git')!r})")
        return None
    real = spec.get("package", alias)
    if "version" in spec:
        return (subject, real, spec["version"], None)
    if spec.get("workspace") is True:
        return _workspace_dependency(ctx, subject, alias, real, inherited)
    if "path" in spec:
        ctx.pass_row("lock-staleness", subject,
                     "path-only, no registry identity")
        return None
    ctx.fail_row("lock-staleness", subject,
                 f"no version, workspace, or path in {spec!r}")
    return None


def _workspace_dependency(ctx, subject, alias, real, inherited):
    base = inherited.get(real, inherited.get(alias))
    if isinstance(base, dict) and isinstance(base.get("package"), str):
        real = base["package"]
    if isinstance(base, str):
        requirement = base
    elif isinstance(base, dict) and "version" in base:
        requirement = base["version"]
    else:
        requirement = None
    if requirement is None:
        ctx.fail_row("lock-staleness", subject,
                     "workspace inheritance unresolvable: "
                     f"[workspace.dependencies] lacks {real!r}")
        return None
    return (subject, real, requirement, "workspace")


def _read_manifests(ctx, inherited):
    manifests = sorted(glob.glob(ctx.path("crates/*/Cargo.toml")))
    if not manifests:
        ctx.fail_row("lock-staleness", "crates/*/Cargo.toml",
                     "no members found")
    members = set()
    declared = []
    for manifest in manifests:
        try:
            with open(manifest, "rb") as handle:
                doc = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as err:
            ctx.fail_row("lock-staleness", manifest, f"unreadable ({err})")
            continue
        package = doc.get("package")
        if not isinstance(package, dict) or not package.get("name"):
            continue
        crate = package["name"]
        members.add(crate)
        for scope, alias, spec in walk_dep_tables(doc, ""):
            item = _declared_dependency(ctx, crate, scope, alias, spec, inherited)
            if item is not None:
                declared.append(item)
    ctx.member_names = members
    by_name = _index_lock(ctx.locked)
    return manifests, declared, by_name


def _index_lock(locked):
    by_name = {}
    for entry in locked:
        by_name.setdefault(entry.get("name"), []).append(entry)
    return by_name


def _check_requirement(ctx, subject, real, requirement, by_name, members):
    if not isinstance(requirement, str) or not requirement.startswith("="):
        ctx.fail_row("lock-staleness", subject,
                     f"requirement {requirement!r} is not exact `=x.y.z` "
                     "(VER-2.26)")
        return False
    want = requirement[1:].split("+", 1)[0]
    matches = [entry for entry in by_name.get(real, [])
               if (entry.get("version") or "").split("+", 1)[0] == want]
    if not matches:
        have = sorted({entry.get("version") or ""
                       for entry in by_name.get(real, [])})
        ctx.fail_row("lock-staleness", subject,
                     f"declared {requirement!r} has no locked identity "
                     f"(locked versions: {have})")
        return False
    return _check_resolved_source(ctx, subject, real, want, requirement,
                                  matches, members)


def _check_resolved_source(ctx, subject, real, want, requirement,
                           matches, members):
    sources = {entry.get("source") or "local" for entry in matches}
    if len(sources) > 1:
        ctx.fail_row("lock-staleness", subject,
                     f"ambiguous identity: {real} {want} resolves from "
                     f"{sorted(sources)}")
        return False
    source = next(iter(sources))
    if real in members and source != "local":
        ctx.fail_row("lock-staleness", subject,
                     f"workspace member {real} locked from {source}")
        return False
    if real in members:
        ctx.pass_row("lock-staleness", subject, f"{requirement} @ workspace")
        return True
    if source != CRATES_IO:
        ctx.fail_row("lock-staleness", subject,
                     f"locked from non-registry source {source}")
        return False
    ctx.pass_row("lock-staleness", subject, f"{requirement} @ registry")
    return True


def _check_declared(ctx, declared, by_name, members):
    passed = failed = 0
    for subject, real, requirement, _ in declared:
        if _check_requirement(ctx, subject, real, requirement, by_name, members):
            passed += 1
        else:
            failed += 1
    ctx.info_row("lock-staleness", "(declared-summary)",
                 f"{passed} match, {failed} fail, "
                 f"{len(by_name)} locked names retained")


def _check_membership(ctx, locked, by_name, members):
    local_names = {entry.get("name") for entry in locked
                   if not entry.get("source")}
    if local_names != members:
        ctx.fail_row("lock-staleness", "(lock-membership)",
                     f"local lock {sorted(local_names)} != "
                     f"members {sorted(members)}")
    else:
        ctx.pass_row("lock-staleness", "(lock-membership)",
                     f"{len(members)} members")
    _check_lock_graph(ctx, locked, by_name, local_names & members)


def _edge_candidates(edge, by_name):
    parts = edge.split(" ")
    candidates = by_name.get(parts[0], [])
    if len(parts) > 1:
        candidates = [entry for entry in candidates
                      if (entry.get("version") or "") == parts[1]]
    if len(parts) > 2:
        source = parts[2].strip("()")
        candidates = [entry for entry in candidates
                      if (entry.get("source") or "") == source]
    return candidates


def _walk_lock_graph(ctx, queue, by_name):
    reachable = {lock_identity(entry) for entry in queue}
    while queue:
        entry = queue.pop()
        for edge in entry.get("dependencies", []) or []:
            candidates = _edge_candidates(edge, by_name)
            if not candidates:
                ctx.fail_row("lock-staleness", "(lock-graph)",
                             f"dangling edge {entry.get('name')} -> {edge}")
            for candidate in candidates:
                identity = lock_identity(candidate)
                if identity not in reachable:
                    reachable.add(identity)
                    queue.append(candidate)
    return reachable


def _check_lock_graph(ctx, locked, by_name, roots):
    queue = [entry for name in sorted(roots) for entry in by_name.get(name, [])]
    reachable = _walk_lock_graph(ctx, queue, by_name)
    stranded = [entry for entry in locked
                if lock_identity(entry) not in reachable]
    if stranded:
        for entry in sorted(stranded, key=lambda row: row.get("name", "")):
            ctx.fail_row("lock-staleness", "(lock-graph)",
                         f"unreachable locked package {entry.get('name')} "
                         f"{entry.get('version')}")
    else:
        ctx.pass_row("lock-staleness", "(lock-graph)",
                     f"{len(locked)} locked packages reachable")


def _check_lock_mtime(ctx, manifests):
    try:
        lock_mtime = os.path.getmtime(ctx.path("Cargo.lock"))
        newest_manifest = max(os.path.getmtime(manifest) for manifest in manifests)
        stale = lock_mtime < newest_manifest
        ctx.info_row("lock-mtime", "Cargo.lock",
                     f"lock_is_newest={str(not stale).lower()}")
    except OSError as err:
        ctx.info_row("lock-mtime", "Cargo.lock", f"mtime unreadable ({err})")


def check_effective_identity(ctx):
    inherited = _read_workspace(ctx)
    ctx.locked = _read_lock(ctx)
    if ctx.locked is None or inherited is None:
        return
    manifests, declared, by_name = _read_manifests(ctx, inherited)
    _check_declared(ctx, declared, by_name, ctx.member_names)
    _check_membership(ctx, ctx.locked, by_name, ctx.member_names)
    _check_lock_mtime(ctx, manifests)
