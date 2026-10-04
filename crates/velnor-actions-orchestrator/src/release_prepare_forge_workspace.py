"""Source-only Cargo workspace discovery, without repository execution."""
import fnmatch
import posixpath


def prepare_member_matches(path, pattern):
    parts, patterns = path.split("/"), pattern.split("/")
    pending, seen = [(0, 0)], set()
    while pending:
        index, token = pending.pop()
        if (index, token) in seen:
            continue
        seen.add((index, token))
        if token == len(patterns):
            if index == len(parts):
                return True
            continue
        if patterns[token] == "**":
            pending.append((index, token + 1))
            if index < len(parts):
                pending.append((index + 1, token))
        elif index < len(parts) and fnmatch.fnmatchcase(parts[index], patterns[token]):
            pending.append((index + 1, token + 1))
    return False


def prepare_literal_prefix(path, prefix):
    pieces = lambda value: tuple(part for part in value.split("/") if part not in ("", "."))
    target, initial = pieces(path), pieces(prefix)
    return target[:len(initial)] == initial


def prepare_workspace_excluded(path, root_path, root):
    workspace, directory = root.get("workspace", {}), posixpath.dirname(root_path)
    excluded = any(prepare_literal_prefix(path, posixpath.join(directory, item))
                   for item in workspace.get("exclude", []))
    explicit = any(prepare_literal_prefix(path, posixpath.join(directory, item))
                   for item in workspace.get("members", []))
    return excluded and not explicit


def prepare_find_workspace_root(approved, requested, entries, documents):
    def inspect(path):
        if path not in documents:
            documents[path] = prepare_manifest(approved, entries[path])
        value = documents[path]
        explicit = value.get("package", {}).get("workspace")
        require(not (explicit is not None and "workspace" in value), "prepare_multiple_workspace_roots")
        if "workspace" in value:
            return None if prepare_workspace_excluded(requested, path, value) else path
        if explicit is None:
            return None
        require(isinstance(explicit, str) and not explicit.startswith("/") and "\\" not in explicit,
                "prepare_package_workspace")
        root = posixpath.normpath(posixpath.join(posixpath.dirname(path), explicit, "Cargo.toml"))
        require(not root.startswith("../") and root in entries, "prepare_workspace_root_missing")
        return root
    found = inspect(requested)
    if found is not None:
        return found
    parent = posixpath.dirname(requested)
    while True:
        cached = posixpath.join(parent, "Cargo.toml")
        if cached in documents and "workspace" in documents[cached]:
            if not prepare_workspace_excluded(requested, cached, documents[cached]):
                return cached
        if not parent:
            break
        parent = posixpath.dirname(parent)
    parent = posixpath.dirname(requested)
    while parent:
        parent = posixpath.dirname(parent)
        if parent == "target/package" or parent.endswith("/target/package"):
            break
        candidate = posixpath.join(parent, "Cargo.toml")
        if candidate in entries:
            found = inspect(candidate)
            if found is not None:
                return found
    return None


def prepare_workspace_root(approved, entries):
    requested = os.environ["RELEASE_MANIFEST"]
    require(isinstance(requested, str) and requested == posixpath.normpath(requested) and
            not requested.startswith(("/", "../")) and "\\" not in requested and
            requested.split("/")[-1] == "Cargo.toml" and requested in entries,
            "prepare_requested_manifest")
    documents = {}
    found = prepare_find_workspace_root(approved, requested, entries, documents)
    root_path = found or requested
    if root_path not in documents:
        documents[root_path] = prepare_manifest(approved, entries[root_path])
    root = documents[root_path]
    require(found is None or "workspace" in root, "prepare_workspace_root_missing")
    return root_path, root


def prepare_publishable(manifest, root, path, entries):
    package = manifest.get("package", {})
    publish = package.get("publish")
    if same_json(publish, {"workspace": True}):
        publish = root.get("workspace", {}).get("package", {}).get("publish")
    if publish is not None:
        require(type(publish) is bool or isinstance(publish, list), "prepare_publish_setting")
        return bool(publish)
    if "lib" in manifest or any(manifest.get(kind) for kind in ("bin", "test", "bench")):
        return True
    parent = path.removesuffix("Cargo.toml")
    build = package.get("build")
    if isinstance(build, str) or build is not False and parent + "build.rs" in entries:
        return True
    for relative in ("src/lib.rs", "src/main.rs"):
        automatic = "autolib" if relative == "src/lib.rs" else "autobins"
        if package.get(automatic, True) and parent + relative in entries:
            return True
    for directory, automatic in (("src/bin/", "autobins"), ("tests/", "autotests"), ("benches/", "autobenches")):
        if not package.get(automatic, True):
            continue
        prefix = parent + directory
        for source in entries:
            if not source.startswith(prefix):
                continue
            parts = source[len(prefix):].split("/")
            if len(parts) == 1 and parts[0].endswith(".rs") or len(parts) == 2 and parts[1] == "main.rs":
                return True
    return False


def prepare_explicit_members(root_path, entries, members):
    directory = posixpath.dirname(root_path)
    directories = {""}
    for path in entries:
        parent = posixpath.dirname(path)
        while parent:
            directories.add(parent)
            parent = posixpath.dirname(parent)
    pending = []
    for member in members:
        pattern = posixpath.normpath(posixpath.join(directory, member))
        matches = sorted(path for path in set(entries) | directories if prepare_member_matches(path or ".", pattern))
        candidates = [path for path in matches if path in directories] if matches else [pattern]
        for candidate in candidates:
            manifest = posixpath.normpath(posixpath.join(candidate, "Cargo.toml"))
            require(manifest in entries, "prepare_workspace_member_missing")
            pending.append((manifest, False))
    return pending


def prepare_local_dependencies(manifest, root, parent, directory):
    if "package" not in manifest:
        return []
    pending = []
    for (table_kind, _), dependencies in _manifest_dependency_tables(manifest).items():
        if table_kind == "workspace":
            continue
        for alias, value in dependencies.items():
            if not isinstance(value, dict):
                continue
            base = parent
            if value.get("workspace") is True:
                inherited = root.get("workspace", {}).get("dependencies", {})
                require(alias in inherited, "prepare_workspace_dependency_missing")
                value, base = inherited[alias], directory
            if not isinstance(value, dict) or "path" not in value:
                continue
            local = value["path"]
            require(isinstance(local, str) and not local.startswith("/") and "\\" not in local,
                    "prepare_dependency_path")
            target = posixpath.normpath(posixpath.join(base, local, "Cargo.toml"))
            require(not target.startswith("../"), "prepare_dependency_outside_repository")
            pending.append((target, True))
    return pending


def prepare_workspace_documents(approved, root_path, root, entries, members, excludes):
    documents = {root_path: root}
    pending = prepare_explicit_members(root_path, entries, members)
    pending.append((root_path, False))
    pending.reverse()
    accepted = {}
    directory = posixpath.dirname(root_path)
    visited = set()
    while pending:
        path, dependency = pending.pop()
        if path in visited:
            continue
        visited.add(path)
        require(path in entries, "prepare_workspace_member_missing")
        inside = not posixpath.relpath(path, directory or ".").startswith("../")
        if dependency and not inside:
            if prepare_find_workspace_root(approved, path, entries, documents) != root_path:
                continue
        if prepare_workspace_excluded(path, root_path, root):
            continue
        if path not in documents:
            documents[path] = prepare_manifest(approved, entries[path])
        manifest = documents[path]
        accepted[path] = manifest
        if "workspace" in root:
            found = prepare_find_workspace_root(approved, path, entries, documents)
            require(found == root_path, "prepare_workspace_member_root")
            parent = posixpath.dirname(path)
            pending.extend(reversed(prepare_local_dependencies(manifest, root, parent, directory)))
    requested = os.environ["RELEASE_MANIFEST"]
    require(requested in accepted, "prepare_requested_manifest_not_member")
    return accepted


def prepare_scope(approved, entries):
    root_path, root = prepare_workspace_root(approved, entries)
    workspace = root.get("workspace", {})
    members, excludes = workspace.get("members", []), workspace.get("exclude", [])
    require(isinstance(members, list) and isinstance(excludes, list) and
            all(isinstance(item, str) for item in members + excludes), "prepare_workspace")
    selected, manifests, publishable = {}, {}, 0
    allowed = {root_path, root_path.removesuffix("Cargo.toml") + "Cargo.lock"}
    documents = prepare_workspace_documents(approved, root_path, root, entries, members, excludes)
    for path, manifest in documents.items():
        name = manifest.get("package", {}).get("name")
        if name is not None:
            require(name not in manifests, "prepare_duplicate_workspace_package")
            manifests[name] = path
            allowed.add(path)
            publishable += prepare_publishable(manifest, root, path, entries)
        if name in approved["packages"]:
            selected[name] = path
    require(set(selected) == set(approved["packages"]), "prepare_package_scope")
    allowed.update(path.removesuffix("Cargo.toml") + "CHANGELOG.md" for path in selected.values())
    return selected, allowed, manifests, publishable > 1, root_path
