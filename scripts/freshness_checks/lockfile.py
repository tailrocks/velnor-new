import glob as globmod
import os
import tomllib

from report import fail_row, info_row, pass_row


DEP_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def walk_dep_tables(node, prefix):
    """Yield (scope, key, spec) for every dependency table under node."""
    found = []
    if not isinstance(node, dict):
        return found
    for section in DEP_SECTIONS:
        table = node.get(section)
        if isinstance(table, dict):
            scope = f"{prefix}{section}" if prefix else section
            for key in sorted(table):
                found.append((scope, key, table[key]))
    target = node.get("target")
    if isinstance(target, dict):
        for name in sorted(target):
            found.extend(walk_dep_tables(target[name], f"target.{name}."))
    return found


def lock_identity(entry):
    """Resolve each lock node by its name+version+source identity."""
    name = entry.get("name", "")
    version = (entry.get("version") or "").split("+", 1)[0]
    source = entry.get("source") or "local"
    return (name, version, source)


def load_workspace(root):
    try:
        with open(f"{root}/Cargo.toml", "rb") as handle:
            workspace_doc = tomllib.load(handle)
        return (workspace_doc.get("workspace") or {}).get("dependencies", {})
    except (OSError, tomllib.TOMLDecodeError) as err:
        fail_row("lock-staleness", "workspace Cargo.toml", f"unreadable ({err})")
        return None


def load_locked(root):
    try:
        with open(f"{root}/Cargo.lock", "rb") as handle:
            return tomllib.load(handle).get("package", [])
    except (OSError, tomllib.TOMLDecodeError, AttributeError) as err:
        fail_row("lock-staleness", "Cargo.lock", f"unreadable ({err})")
        return None


def declared_for_manifest(manifest, inherited, member_names):
    try:
        with open(manifest, "rb") as handle:
            doc = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        fail_row("lock-staleness", manifest, f"unreadable ({err})")
        return []
    package = doc.get("package")
    if not isinstance(package, dict) or not package.get("name"):
        return []
    crate = package["name"]
    member_names.add(crate)
    declared = []
    for scope, alias, spec in walk_dep_tables(doc, ""):
        subject = f"{crate}:{scope}:{alias}"
        if isinstance(spec, str):
            declared.append((subject, alias, spec, None))
        elif isinstance(spec, dict):
            append_table_dependency(subject, alias, spec, inherited, declared)
        else:
            fail_row("lock-staleness", subject, f"malformed spec {spec!r}")
    return declared


def append_table_dependency(subject, alias, spec, inherited, declared):
    if spec.get("git"):
        fail_row("lock-staleness", subject,
                 f"git dependency forbidden ({spec.get('git')!r})")
        return
    real = spec.get("package", alias)
    if "version" in spec:
        declared.append((subject, real, spec["version"], None))
        return
    if spec.get("workspace") is True:
        append_workspace_dependency(subject, alias, real, inherited, declared)
        return
    if "path" in spec:
        pass_row("lock-staleness", subject,
                 "path-only, no registry identity")
        return
    fail_row("lock-staleness", subject,
             f"no version, workspace, or path in {spec!r}")


def append_workspace_dependency(subject, alias, real, inherited, declared):
    base = inherited.get(real, inherited.get(alias))
    if isinstance(base, dict) and isinstance(base.get("package"), str):
        real = base["package"]
    req = None
    if isinstance(base, str):
        req = base
    elif isinstance(base, dict) and "version" in base:
        req = base["version"]
    if req is None:
        fail_row("lock-staleness", subject,
                 "workspace inheritance unresolvable: "
                 f"[workspace.dependencies] lacks {real!r}")
        return
    declared.append((subject, real, req, "workspace"))


def collect_declared(root, inherited):
    manifests = sorted(globmod.glob(f"{root}/crates/*/Cargo.toml"))
    if not manifests:
        fail_row("lock-staleness", "crates/*/Cargo.toml", "no members found")
    member_names = set()
    declared = []
    for manifest in manifests:
        declared.extend(declared_for_manifest(manifest, inherited, member_names))
    return manifests, member_names, declared


def check_declared_identities(declared, locked, member_names):
    by_name = {}
    for entry in locked:
        by_name.setdefault(entry.get("name"), []).append(entry)
    passed = failed = 0
    for subject, real, req, _ in declared:
        if check_one_identity(subject, real, req, by_name, member_names):
            passed += 1
        else:
            failed += 1
    info_row("lock-staleness", "(declared-summary)",
             f"{passed} match, {failed} fail, "
             f"{len(by_name)} locked names retained")
    return by_name


def check_one_identity(subject, real, req, by_name, member_names):
    if not isinstance(req, str) or not req.startswith("="):
        tick = chr(96)
        fail_row("lock-staleness", subject,
                 f"requirement {req!r} is not exact {tick}=x.y.z{tick} (VER-2.26)")
        return False
    want = req[1:].split("+", 1)[0]
    matches = [entry for entry in by_name.get(real, [])
               if (entry.get("version") or "").split("+", 1)[0] == want]
    if not matches:
        have = sorted({(entry.get("version") or "")
                       for entry in by_name.get(real, [])})
        fail_row("lock-staleness", subject,
                 f"declared {req!r} has no locked identity "
                 f"(locked versions: {have})")
        return False
    sources = {entry.get("source") or "local" for entry in matches}
    if len(sources) > 1:
        fail_row("lock-staleness", subject,
                 f"ambiguous identity: {real} {want} resolves from "
                 f"{sorted(sources)}")
        return False
    source = next(iter(sources))
    if real in member_names:
        if source != "local":
            fail_row("lock-staleness", subject,
                     f"workspace member {real} locked from {source}")
            return False
        pass_row("lock-staleness", subject, f"{req} @ workspace")
        return True
    if source != CRATES_IO:
        fail_row("lock-staleness", subject,
                 f"locked from non-registry source {source}")
        return False
    pass_row("lock-staleness", subject, f"{req} @ registry")
    return True


def verify_lock_membership(locked, member_names):
    local_names = {entry.get("name") for entry in locked
                   if not entry.get("source")}
    if local_names != member_names:
        fail_row("lock-staleness", "(lock-membership)",
                 f"local lock {sorted(local_names)} != "
                 f"members {sorted(member_names)}")
    else:
        pass_row("lock-staleness", "(lock-membership)",
                 f"{len(member_names)} members")
    return local_names


def verify_lock_graph(locked, local_names, member_names, by_name):
    queue = [entry for name in sorted(local_names & member_names)
             for entry in by_name.get(name, [])]
    reachable = {lock_identity(entry) for entry in queue}
    while queue:
        entry = queue.pop()
        add_reachable_dependencies(entry, by_name, reachable, queue)
    stranded = [entry for entry in locked
                if lock_identity(entry) not in reachable]
    if stranded:
        for entry in sorted(stranded, key=lambda item: item.get("name", "")):
            fail_row("lock-staleness", "(lock-graph)",
                     f"unreachable locked package "
                     f"{entry.get('name')} {entry.get('version')}")
    else:
        pass_row("lock-staleness", "(lock-graph)",
                 f"{len(locked)} locked packages reachable")


def add_reachable_dependencies(entry, by_name, reachable, queue):
    for edge in entry.get("dependencies", []) or []:
        parts = edge.split(" ")
        cands = by_name.get(parts[0], [])
        if len(parts) > 1:
            cands = [candidate for candidate in cands
                     if (candidate.get("version") or "") == parts[1]]
        if len(parts) > 2:
            want_src = parts[2].strip("()")
            cands = [candidate for candidate in cands
                     if (candidate.get("source") or "") == want_src]
        if not cands:
            fail_row("lock-staleness", "(lock-graph)",
                     f"dangling edge {entry.get('name')} -> {edge}")
            continue
        for candidate in cands:
            ident = lock_identity(candidate)
            if ident not in reachable:
                reachable.add(ident)
                queue.append(candidate)


def verify_lock_mtime(root, manifests):
    try:
        lock_mtime = os.path.getmtime(f"{root}/Cargo.lock")
        newest_manifest = max(os.path.getmtime(manifest) for manifest in manifests)
        stale = lock_mtime < newest_manifest
        info_row("lock-mtime", "Cargo.lock",
                 f"lock_is_newest={str(not stale).lower()}")
    except OSError as err:
        info_row("lock-mtime", "Cargo.lock", f"mtime unreadable ({err})")


def run_lock_checks(root):
    inherited = load_workspace(root)
    locked = load_locked(root)
    if locked is not None and inherited is not None:
        manifests, member_names, declared = collect_declared(root, inherited)
        by_name = check_declared_identities(declared, locked, member_names)
        local_names = verify_lock_membership(locked, member_names)
        verify_lock_graph(locked, local_names, member_names, by_name)
        verify_lock_mtime(root, manifests)
    return locked
