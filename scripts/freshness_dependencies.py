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


def _load_manifest(ctx, relative):
    path = ctx.path(relative)
    try:
        with open(path, "rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        ctx.fail_row("lock-staleness", relative, f"unreadable ({err})")
        return None


def _excluded_workspaces(ctx, relative, workspace):
    excludes = workspace.get("exclude", [])
    if not isinstance(excludes, list):
        ctx.fail_row("lock-staleness", relative or "Cargo.toml",
                     "workspace.exclude must be an array")
        return []
    discovered = []
    base = os.path.join(ctx.root, relative)
    for excluded in excludes:
        if not isinstance(excluded, str):
            ctx.fail_row("lock-staleness", relative or "Cargo.toml",
                         f"workspace.exclude entry is not a string: {excluded!r}")
            continue
        pattern = os.path.join(base, excluded)
        for hit in glob.glob(pattern, recursive=True):
            manifest = hit if os.path.basename(hit) == "Cargo.toml" \
                else os.path.join(hit, "Cargo.toml")
            if not os.path.isfile(manifest):
                continue
            nested_relative = os.path.relpath(os.path.dirname(manifest), ctx.root)
            nested = _load_manifest(ctx, os.path.relpath(manifest, ctx.root))
            if nested and isinstance(nested.get("workspace"), dict):
                discovered.append((nested_relative, nested))
    return discovered


def _discover_workspaces(ctx):
    pending = [("", _load_manifest(ctx, "Cargo.toml"))]
    roots = []
    seen = set()
    while pending:
        relative, document = pending.pop(0)
        if relative in seen or document is None:
            continue
        seen.add(relative)
        workspace = document.get("workspace")
        if not isinstance(workspace, dict):
            ctx.fail_row("lock-staleness", relative or "Cargo.toml",
                         "no [workspace] table")
            continue
        roots.append((relative, document))
        pending.extend(_excluded_workspaces(ctx, relative, workspace))
    return roots


def _read_lock(ctx, relative):
    lock_path = os.path.join(relative, "Cargo.lock") if relative else "Cargo.lock"
    try:
        with open(ctx.path(lock_path), "rb") as handle:
            return tomllib.load(handle).get("package", [])
    except (OSError, tomllib.TOMLDecodeError, AttributeError) as err:
        ctx.fail_row("lock-staleness", lock_path, f"unreadable ({err})")
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


def _member_manifests(ctx, relative, document):
    workspace_dir = os.path.join(ctx.root, relative)
    manifests = []
    if isinstance(document.get("package"), dict):
        manifests.append(os.path.join(workspace_dir, "Cargo.toml"))
    members = (document.get("workspace") or {}).get("members", [])
    if not isinstance(members, list):
        ctx.fail_row("lock-staleness", f"{relative or '.'}/Cargo.toml",
                     "workspace.members must be an array")
        members = []
    for member in members:
        if not isinstance(member, str):
            ctx.fail_row("lock-staleness", f"{relative or '.'}/Cargo.toml",
                         f"workspace member is not a string: {member!r}")
            continue
        pattern = os.path.join(workspace_dir, member, "Cargo.toml")
        matches = glob.glob(pattern, recursive=True)
        if not matches:
            ctx.fail_row("lock-staleness", f"{relative or '.'}/{member}",
                         "workspace member manifest not found")
        manifests.extend(matches)
    return sorted(set(manifests))


def _read_manifests(ctx, manifests, inherited):
    members = set()
    declared = []
    for manifest in manifests:
        try:
            with open(manifest, "rb") as handle:
                document = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as err:
            ctx.fail_row("lock-staleness", manifest, f"unreadable ({err})")
            continue
        package = document.get("package")
        if not isinstance(package, dict) or not package.get("name"):
            ctx.fail_row("lock-staleness", manifest,
                         "workspace member has no package name")
            continue
        crate = package["name"]
        members.add(crate)
        for scope, alias, spec in walk_dep_tables(document, ""):
            item = _declared_dependency(ctx, crate, scope, alias, spec, inherited)
            if item is not None:
                declared.append(item)
    return members, declared


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
    return _check_resolved_source(ctx, subject, real, requirement, matches,
                                  members)


def _check_resolved_source(ctx, subject, real, requirement, matches, members):
    sources = {entry.get("source") or "local" for entry in matches}
    if len(sources) > 1:
        ctx.fail_row("lock-staleness", subject,
                     f"ambiguous identity: {real} {requirement[1:]} "
                     f"resolves from {sorted(sources)}")
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


def _check_declared(ctx, relative, declared, by_name, members):
    passed = failed = 0
    for subject, real, requirement, _ in declared:
        if _check_requirement(ctx, subject, real, requirement, by_name, members):
            passed += 1
        else:
            failed += 1
    subject = f"{relative}/(declared-summary)" if relative \
        else "(declared-summary)"
    ctx.info_row("lock-staleness", subject,
                 f"{passed} match, {failed} fail, "
                 f"{len(by_name)} locked names retained")


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


def _check_lock_graph(ctx, relative, locked, by_name, members):
    local = {entry.get("name") for entry in locked if not entry.get("source")}
    queue = [entry for name in sorted(local & members)
             for entry in by_name.get(name, [])]
    reachable = {lock_identity(entry) for entry in queue}
    while queue:
        entry = queue.pop()
        for edge in entry.get("dependencies", []) or []:
            candidates = _edge_candidates(edge, by_name)
            if not candidates:
                subject = f"{relative}/(lock-graph)" if relative \
                    else "(lock-graph)"
                ctx.fail_row("lock-staleness", subject,
                             f"dangling edge {entry.get('name')} -> {edge}")
            for candidate in candidates:
                identity = lock_identity(candidate)
                if identity not in reachable:
                    reachable.add(identity)
                    queue.append(candidate)
    stranded = [entry for entry in locked
                if lock_identity(entry) not in reachable]
    subject = f"{relative}/(lock-graph)" if relative else "(lock-graph)"
    if stranded:
        for entry in sorted(stranded, key=lambda row: row.get("name", "")):
            ctx.fail_row("lock-staleness", subject,
                         f"unreachable locked package {entry.get('name')} "
                         f"{entry.get('version')}")
    else:
        ctx.pass_row("lock-staleness", subject,
                     f"{len(locked)} locked packages reachable")


def _check_workspace(ctx, relative, document):
    label = relative or "."
    workspace_dir = os.path.join(ctx.root, relative)
    workspace = document.get("workspace") or {}
    inherited = workspace.get("dependencies", {})
    if not isinstance(inherited, dict):
        ctx.fail_row("lock-staleness", f"{label}/Cargo.toml",
                     "workspace.dependencies must be a table")
        return []
    locked = _read_lock(ctx, relative)
    if locked is None:
        return []
    manifests = _member_manifests(ctx, relative, document)
    if not manifests:
        ctx.fail_row("lock-staleness", f"{label}/Cargo.toml",
                     "no workspace member manifests found")
    members, declared = _read_manifests(ctx, manifests, inherited)
    by_name = _index_lock(locked)
    _check_declared(ctx, relative, declared, by_name, members)
    local_names = {entry.get("name") for entry in locked
                   if not entry.get("source")}
    membership = f"{relative}/(lock-membership)" if relative \
        else "(lock-membership)"
    if local_names != members:
        ctx.fail_row("lock-staleness", membership,
                     f"local lock {sorted(local_names)} != members "
                     f"{sorted(members)}")
    else:
        ctx.pass_row("lock-staleness", membership, f"{len(members)} members")
    _check_lock_graph(ctx, relative, locked, by_name, members)
    lock_path = os.path.join(relative, "Cargo.lock") if relative else "Cargo.lock"
    _check_lock_mtime(ctx, lock_path, manifests)
    return locked


def _check_lock_mtime(ctx, lock_path, manifests):
    try:
        lock_mtime = os.path.getmtime(ctx.path(lock_path))
        newest_manifest = max(os.path.getmtime(manifest) for manifest in manifests)
        stale = lock_mtime < newest_manifest
        ctx.info_row("lock-mtime", lock_path,
                     f"lock_is_newest={str(not stale).lower()}")
    except (OSError, ValueError) as err:
        ctx.info_row("lock-mtime", lock_path, f"mtime unreadable ({err})")


def check_effective_identity(ctx):
    ctx.workspace_roots = _discover_workspaces(ctx)
    ctx.locked = []
    ctx.member_names = set()
    for relative, document in ctx.workspace_roots:
        locked = _check_workspace(ctx, relative, document)
        ctx.locked.extend(locked)
