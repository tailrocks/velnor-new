"""Read-only source and typed proposal proofs for release PR preparation."""
import base64
import posixpath
import tomllib


def prepare_source_tree(approved):
    repository = approved["repository"]
    commit = forge_api(f"repos/{repository}/git/commits/{approved['source_sha']}")
    require(isinstance(commit, dict) and commit.get("sha") == approved["source_sha"],
            "prepare_source_commit")
    tree_sha = commit.get("tree", {}).get("sha")
    require(isinstance(tree_sha, str) and re.fullmatch(r"[0-9a-f]{40}", tree_sha),
            "prepare_source_tree")
    return tree_sha, prepare_tree(approved, tree_sha)


def prepare_tree(approved, sha):
    tree = forge_api(f"repos/{approved['repository']}/git/trees/{sha}?recursive=1")
    require(isinstance(tree, dict) and tree.get("sha") == sha and
            tree.get("truncated") is False and isinstance(tree.get("tree"), list),
            "prepare_tree_incomplete")
    entries, seen = {}, set()
    for entry in tree["tree"]:
        path = entry.get("path")
        require(isinstance(path, str) and path not in seen and not path.startswith("/") and
                all(part not in ("", ".", "..") for part in path.split("/")) and
                re.fullmatch(r"[0-9a-f]{40}", entry.get("sha", "")), "prepare_tree_entry")
        seen.add(path)
        require((entry.get("type"), entry.get("mode")) in {
            ("tree", "040000"), ("blob", "100644"), ("blob", "100755"),
            ("blob", "120000"), ("commit", "160000")}, "prepare_tree_mode")
        if entry.get("type") != "tree":
            entries[path] = {key: entry.get(key) for key in ("sha", "mode", "type")}
    return entries


def prepare_source_blob(approved, entry):
    require(entry.get("type") == "blob" and entry.get("mode") == "100644", "prepare_manifest_mode")
    blob = forge_api(f"repos/{approved['repository']}/git/blobs/{entry['sha']}")
    require(isinstance(blob, dict) and blob.get("sha") == entry["sha"] and
            blob.get("encoding") == "base64", "prepare_manifest_blob")
    raw = base64.b64decode(blob.get("content", "").replace("\n", ""), validate=True)
    require(len(raw) <= 1024 * 1024 and prepare_blob_sha(raw) == entry["sha"],
            "prepare_manifest_digest")
    return raw


def prepare_manifest(approved, entry):
    return tomllib.loads(prepare_source_blob(approved, entry).decode("utf-8"))


def prepare_blob_sha(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()


def prepare_proposal(approved, base, tree_sha, entries):
    run_id, attempt = os.environ["GITHUB_RUN_ID"], os.environ["GITHUB_RUN_ATTEMPT"]
    name = f"velnor-release-proposal-r{run_id}-a{attempt}"
    raw, receipt = artifact_evidence(approved, name, "release-preparation-source")
    identifier = os.environ.get("RELEASE_PREPARE_ARTIFACT_ID", "")
    digest = os.environ.get("RELEASE_PREPARE_ARTIFACT_DIGEST", "")
    require(re.fullmatch(r"[1-9][0-9]*", identifier) and re.fullmatch(r"[0-9a-f]{64}", digest) and
            receipt.get("id") == int(identifier) and receipt.get("digest") == "sha256:" + digest and
            receipt.get("name") == name, "prepare_artifact_binding")
    proposal = decode_json(raw)
    require(isinstance(proposal, dict) and set(proposal) == {
        "schema", "policy", "source_sha", "workflow_sha", "run_id", "run_attempt", "actor",
        "base", "files", "source_tree", "packages", "status", "workspace_manifest", "manifests"}, "prepare_proposal_fields")
    require(type(proposal["schema"]) is int, "prepare_proposal_schema")
    expected = {"schema": 1, "policy": approved, "source_sha": approved["source_sha"],
                "workflow_sha": os.environ["GITHUB_SHA"], "run_id": run_id,
                "run_attempt": attempt, "actor": os.environ["GITHUB_ACTOR_ID"],
                "base": base, "source_tree": tree_sha, "status": "prepared"}
    require(all(same_json(proposal.get(key), value) for key, value in expected.items()), "prepare_proposal_identity")
    packages = proposal["packages"]
    require(isinstance(packages, dict) and set(packages) == set(approved["packages"]), "prepare_proposal_packages")
    for package, details in packages.items():
        require(isinstance(details, dict) and set(details) == {
            "version", "notes", "previous_version", "semver_check", "breaking_changes"} and
                isinstance(details["version"], str) and VERSION.fullmatch(details["version"]) and
                isinstance(details["notes"], str) and len(details["notes"]) <= 65536 and
                isinstance(details["previous_version"], str) and VERSION.fullmatch(details["previous_version"]) and
                details["semver_check"] in ("compatible", "incompatible", "skipped", "unknown") and
                isinstance(details["breaking_changes"], str) and len(details["breaking_changes"]) <= 32768 and
                bool(details["breaking_changes"]) == (details["semver_check"] == "incompatible"),
                "prepare_proposal_package")
    selected, allowed, manifests, multiple_public, root_path = prepare_scope(approved, entries)
    require(proposal["workspace_manifest"] == root_path and proposal["manifests"] == manifests,
            "prepare_proposal_workspace")
    changes = prepare_changes(approved, proposal["files"], allowed, entries)
    prepare_versions(approved, selected, manifests, entries, changes, packages, root_path)
    return proposal, receipt, changes, multiple_public


def prepare_changes(approved, files, allowed, entries):
    require(isinstance(files, dict) and len(files) <= len(allowed), "prepare_proposal_files")
    changes = {}
    for path, value in files.items():
        require(path in allowed and isinstance(value, dict) and set(value) == {"before", "after", "sha256"},
                "prepare_proposal_path")
        previous = entries.get(path)
        require(previous is None or previous["type"] == "blob" and previous["mode"] == "100644",
                "prepare_file_mode")
        require(value["before"] == (previous["sha"] if previous else None), "prepare_before_changed")
        require(isinstance(value["after"], str) and len(value["after"]) <= 8 * 1024 * 1024,
                "prepare_file_size")
        raw = base64.b64decode(value["after"], validate=True)
        require(hashlib.sha256(raw).hexdigest() == value["sha256"], "prepare_file_digest")
        before = prepare_source_blob(approved, previous) if previous else None
        preparation_bytes(before, raw, path, set(approved["packages"]))
        sha = prepare_blob_sha(raw)
        require(previous is None or sha != previous["sha"], "prepare_unchanged_file")
        changes[path] = {"raw": raw, "sha": sha, "mode": "100644", "type": "blob"}
    return changes


def prepare_version(root, manifest, name):
    package = manifest.get("package", {})
    require(package.get("name") == name, "prepare_package_renamed")
    version = package.get("version")
    if same_json(version, {"workspace": True}):
        version = root.get("workspace", {}).get("package", {}).get("version")
    return version


def prepare_versions(approved, selected, manifests, entries, changes, packages, root_path):
    before_root = prepare_manifest(approved, entries[root_path])
    after_root = (tomllib.loads(changes[root_path]["raw"].decode()) if root_path in changes
                  else before_root)
    inherited, after_versions = False, {}
    root_raw = prepare_source_blob(approved, entries[root_path])
    before_bytes = {root_path: root_raw}
    after_bytes = {root_path: changes.get(root_path, {}).get("raw", root_raw)}
    for name, path in manifests.items():
        before_raw = prepare_source_blob(approved, entries[path])
        after_raw = changes[path]["raw"] if path in changes else before_raw
        before_bytes[path], after_bytes[path] = before_raw, after_raw
        before, after = tomllib.loads(before_raw.decode()), tomllib.loads(after_raw.decode())
        after_versions[name] = prepare_version(after_root, after, name)
        if name not in selected:
            require(prepare_version(before_root, before, name) == prepare_version(after_root, after, name),
                    "prepare_unselected_package_version")
            continue
        inherited = inherited or same_json(before.get("package", {}).get("version"), {"workspace": True})
        require(prepare_version(before_root, before, name) == approved["packages"][name],
                "prepare_source_package_version")
        require(prepare_version(after_root, after, name) == packages[name]["version"],
                "prepare_package_version")
        changelog = path.removesuffix("Cargo.toml") + "CHANGELOG.md"
        notes = preparation_notes(changes[changelog]["raw"], packages[name]["version"]) if changelog in changes else ""
        require(packages[name]["notes"] == notes, "prepare_package_notes")
    if not inherited:
        require(before_root.get("workspace", {}).get("package", {}).get("version") ==
                after_root.get("workspace", {}).get("package", {}).get("version"),
                "prepare_unused_workspace_version")
    lock_path = root_path.removesuffix("Cargo.toml") + "Cargo.lock"
    lock = changes.get(lock_path, {}).get("raw")
    if lock is None:
        require(lock_path in entries, "prepare_source_lock_missing")
        lock = prepare_source_blob(approved, entries[lock_path])
    preparation_lock_versions(lock, after_versions)
    preparation_dependency_versions(before_bytes, after_bytes, after_versions)


def prepare_pr_body(approved, proposal, multiple_public):
    marker = f"<!-- velnor-release:{approved['intent_id']} source:{approved['source_sha']} -->"
    packages = {name: details for name, details in proposal["packages"].items()
                if details["version"] != approved["packages"][name] or details["notes"] or details["breaking_changes"]}
    require(packages, "prepare_no_updated_packages")
    notes = "\n\n".join(
        f"## {name}: {details['previous_version']} to {details['version']}\n\n"
        f"Semver check: {details['semver_check']}.\n\n{details['notes']}"
        + ("\n\n### Breaking changes\n\n" + details["breaking_changes"] if details["breaking_changes"] else "")
        for name, details in sorted(packages.items()))
    body = marker + "\n\n" + notes
    require(len(body) <= 65536, "prepare_pr_body_size")
    versions = {details["version"] for details in packages.values()}
    title = "chore: release"
    if len(packages) == 1 and multiple_public:
        name = next(iter(packages))
        title = f"chore({name}): release v{packages[name]['version']}"
    elif len(versions) == 1:
        title += " v" + next(iter(versions))
    return title, body


