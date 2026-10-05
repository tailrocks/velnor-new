from pathlib import Path
import hashlib

base = Path("/root/.velnor-generator-preview-20261005")

audit = (base / "preseed-preview-git-config-audit.py").read_text()
old = 'ORIGIN = "https://github.com/tailrocks/velnor-new.git"'
if audit.count(old) != 1:
    raise SystemExit("unexpected source audit baseline")
audit = audit.replace(old, 'ORIGIN = "https://github.com/jackin-project/jackin.git"')
audit = audit.replace("Velnor repository origin is absent", "canonical Jackin repository origin is absent")
audit_path = base / "jackin-source-git-config-audit-b80-v1.py"
with audit_path.open("x", encoding="utf-8") as output:
    output.write(audit)
audit_path.chmod(0o644)
audit_sha = hashlib.sha256(audit_path.read_bytes()).hexdigest()

materializer = (base / "preseed-preview-source-materialize.py").read_text()
replacements = {
    'SOURCE = TASK + "/main-source-ac3ab"': 'SOURCE = TASK + "/jackin-source-b80f991d56c2-exact-v2"',
    'MANIFEST = TASK + "/main-source-ac3ab-manifest.json"': 'MANIFEST = TASK + "/jackin-source-b80f991d56c2-tree-manifest.json"',
    'AUDIT = TASK + "/preseed-preview-git-config-audit.py"': 'AUDIT = TASK + "/jackin-source-git-config-audit-b80-v1.py"',
    'AUDIT_SHA256 = "0e5202f10a47c69775513e34a711c07e7be55c4d0d3ddce8f7e81f8831e54c03"': None,
    'COMMIT = "ac3ab6a3d5ba3701c1300bbdd8114390093c5c29"': 'COMMIT = "b80f991d56c2c13f913bb444f9f05a36b16d37df"',
    'TREE = "b4cc6aa0df400e111cbee27a2cd5edc9f0144bac"': 'TREE = "15a71f1adebd75dc217aa3a67fd7e0b78a38b2b6"',
    'ORIGIN = "https://github.com/tailrocks/velnor-new.git"': 'ORIGIN = "https://github.com/jackin-project/jackin.git"',
    "source-materialize-ac3ab-attempt-2.log": "jackin-source-materialize-b80f991d56c2-attempt-1.log",
    "source-partial-verify-ac3ab.log": "jackin-source-partial-verify-b80f991d56c2.log",
    "source-partial-finalize-ac3ab.log": "jackin-source-partial-finalize-b80f991d56c2.log",
    "source-verify-ac3ab.log": "jackin-source-verify-b80f991d56c2.log",
    'if git(["rev-parse", "--abbrev-ref", "origin/HEAD"]).decode().strip() != "origin/main":\n        fail("origin/HEAD does not identify the current main branch")': 'if git(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).decode().strip() != "origin/refactor/build-split-dco":\n        fail("source upstream does not identify the selected canonical task branch")',
    "source commit differs from the pinned main tip": "source commit differs from the pinned task-branch selection",
    "source tree differs from the pinned main tip": "source tree differs from the pinned task-branch selection",
    "immutable Git tip": "immutable Git selection",
}
for old, new in replacements.items():
    count = materializer.count(old)
    if count != 1:
        raise SystemExit("unexpected source materializer baseline: " + old + " count=" + str(count))
    if new is not None:
        materializer = materializer.replace(old, new)
    else:
        materializer = materializer.replace(
            old,
            'AUDIT_SHA256 = "' + audit_sha + '"',
        )

old = 'TREE = "15a71f1adebd75dc217aa3a67fd7e0b78a38b2b6"\nORIGIN = "https://github.com/jackin-project/jackin.git"'
new = '''TREE = "15a71f1adebd75dc217aa3a67fd7e0b78a38b2b6"
PARENT = "3bee4c7217837bc37b928840eb93e717cc2bbfbe"
BRANCH = "refactor/build-split-dco"
PROFILE_PATH = TASK + "/jackin-source-profile-b80f991d56c2.json"
PROFILE_SHA256 = "ad15804b9a0d3a8abd28ad445b22fcad9a3769f0c1417fbc381dc9ebceb893bc"
TRANSCRIPT_PATH = TASK + "/jackin-source-b80f991d56c2-exact-v2-fetch.json"
TRANSCRIPT_SHA256 = "1132d846aebe3111650c7da5f419eb98b0e6fd52dd4c35bc35bb2f4496bb442d"
ORIGIN = "https://github.com/jackin-project/jackin.git"'''
if materializer.count(old) != 1:
    raise SystemExit("unexpected source provenance insertion point")
materializer = materializer.replace(old, new)

needle = "def verify_index(entries):\n"
addition = '''def verify_source_provenance():
    expected = {
        PROFILE_PATH: PROFILE_SHA256,
        TRANSCRIPT_PATH: TRANSCRIPT_SHA256,
    }
    parsed = {}
    for path, digest in expected.items():
        if hash_regular(path) != digest:
            fail("source provenance file digest changed")
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            st = os.fstat(fd)
            if stat.S_IMODE(st.st_mode) != 0o600 or st.st_size > 16384:
                fail("source provenance file mode or size changed")
            data = bytearray()
            while len(data) <= 16384:
                chunk = os.read(fd, min(4096, 16385 - len(data)))
                if not chunk:
                    break
                data.extend(chunk)
        finally:
            os.close(fd)
        try:
            parsed[path] = json.loads(bytes(data))
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("source provenance file is not valid bounded JSON")
    profile = parsed[PROFILE_PATH]
    transcript = parsed[TRANSCRIPT_PATH]
    if (profile.get("repository_id"), profile.get("repository"), profile.get("origin"), profile.get("branch"), profile.get("commit"), profile.get("tree"), profile.get("parent")) != (
        1197700841, "jackin-project/jackin", ORIGIN, BRANCH, COMMIT, TREE, PARENT
    ):
        fail("source profile identity differs from the exact selected task commit")
    if profile.get("signature_verified") is not False:
        fail("source profile signature disposition changed")
    if (transcript.get("repository_id"), transcript.get("origin"), transcript.get("branch"), transcript.get("requested_commit"), transcript.get("commit"), transcript.get("tree"), transcript.get("parent"), transcript.get("profile_sha256")) != (
        1197700841, ORIGIN, BRANCH, COMMIT, COMMIT, TREE, PARENT, PROFILE_SHA256
    ):
        fail("source fetch transcript identity differs from the exact selected commit")
    if transcript.get("checkout_filters_or_hooks") != "none; no checkout, template disabled, hooks path /dev/null":
        fail("source fetch did not preserve the no-checkout policy")


def parse_raw_commit_identity(raw):
    if not isinstance(raw, bytes) or len(raw) > (1 << 20):
        fail("raw Git commit exceeds its fixed limit")
    header, separator, _message = raw.partition(b"\\n\\n")
    if not separator:
        fail("raw Git commit header is malformed")
    trees = [line[5:].decode("ascii", "strict") for line in header.splitlines() if line.startswith(b"tree ")]
    parents = [line[7:].decode("ascii", "strict") for line in header.splitlines() if line.startswith(b"parent ")]
    if len(trees) != 1 or len(parents) != 1 or (trees[0], parents[0]) != (TREE, PARENT):
        fail("raw Git commit tree or parent differs from the reviewed profile")

'''
if materializer.count(needle) != 1:
    raise SystemExit("unexpected materializer validation insertion point")
materializer = materializer.replace(needle, addition + needle)

marker = '    if git(["rev-parse", "HEAD^{tree}"]).decode().strip() != TREE:\n        fail("source tree differs from the pinned task-branch selection")\n'
if materializer.count(marker) != 1:
    raise SystemExit("unexpected source identity insertion point")
materializer = materializer.replace(marker, marker + '    parse_raw_commit_identity(git(["cat-file", "commit", COMMIT], limit=1 << 20))\n    verify_source_provenance()\n')

marker = '        "repository": ORIGIN,\n        "commit": COMMIT,\n        "tree": TREE,\n'
if materializer.count(marker) != 1:
    raise SystemExit("unexpected source manifest insertion point")
materializer = materializer.replace(marker, marker + '        "branch": BRANCH,\n        "parent": PARENT,\n        "profile_sha256": PROFILE_SHA256,\n        "fetch_transcript_sha256": TRANSCRIPT_SHA256,\n')
materializer = materializer.replace('"PASS: local Git config allowlist; "', '"PASS: canonical Jackin Git config allowlist; "')

materializer_path = base / "jackin-source-materialize-b80-v1.py"
with materializer_path.open("x", encoding="utf-8") as output:
    output.write(materializer)
materializer_path.chmod(0o600)
print("audit_sha256=" + audit_sha)
print("materializer_sha256=" + hashlib.sha256(materializer_path.read_bytes()).hexdigest())
