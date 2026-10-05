import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import stat
import sys

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
ROOTFS = TASK / 'rootfs'
BASE_MATERIALIZER = TASK / 'preseed-preview-materialize.py'
DELTA_SCRIPT = TASK / 'preseed-preview-rootfs-input-delta.py'
DELTA_MANIFEST = TASK / 'rootfs-input-delta-manifest.json'
BASE_MANIFEST = TASK / 'rootfs-base-manifest.json'
PLAN = TASK / 'rootfs-stage-plan-with-input.json'
PROVENANCE = TASK / 'official-tool-asset-provenance.json'
STAGE_RECORD = TASK / 'base-stage-attempt-3.json'
PREFLIGHT_RESULT = TASK / 'rootfs-input-delta-preflight-result.json'
LOG = TASK / 'rootfs-input-delta.log'
RESULT = TASK / 'rootfs-input-delta-verify-result.json'
EXPECTED_DELTA = '40e2acf6f621888e3a6f645dc86755d4f5e338d22b5ae9bba53b4f037967377a'
EXPECTED_BASE_MATERIALIZER = '4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3'
EXPECTED_BASE_TREE = '1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763'
EXPECTED_RESULT_TREE = '117843b6eb5d4d06f2309fa205cc51e679445c1dc6f230461e4bd82aa7bf0684'
EXPECTED_BASE_MANIFEST = '1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf'
EXPECTED_PLAN = '4c4a18dd85d71aaa9c9fc9352da9553fc2eb7f5c2d10a8365058c19ea04597bc'
EXPECTED_PROVENANCE = '67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137'
EXPECTED_STAGE_RECORD = '89b9feb8b946c8d435e3978395db69b632f23769e18da41fcfc5e57b9afb37e4'
EXPECTED_PREFLIGHT = '4e22759aacda74146c1852ec15a8deda58b27a1c4654020167768cef66030003'
EXPECTED_LOG = 'c12241e350cfb326960d3110fcfd8352d7d4f302ae1cb73e483184bf515122ad'
EXPECTED_DELTA_MANIFEST = 'dd5425fceb1524fc304e0da46cf6c64a8b63ee643f8ad6e6689cb15c241ef974'

assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site
assert os.geteuid() == 0 and os.getegid() == 0 and all(group == 0 for group in os.getgroups())
assert os.uname().nodename.split('.', 1)[0] == 'bastion'
os.umask(0o077)

def digest_file(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        assert stat.S_ISREG(st.st_mode) and (st.st_uid, st.st_gid) == (0, 0) and st.st_nlink == 1
        hasher = hashlib.sha256()
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            hasher.update(block)
        return hasher.hexdigest()
    finally:
        os.close(fd)

assert digest_file(DELTA_SCRIPT) == EXPECTED_DELTA
assert digest_file(BASE_MATERIALIZER) == EXPECTED_BASE_MATERIALIZER
assert digest_file(BASE_MANIFEST) == EXPECTED_BASE_MANIFEST
assert digest_file(PLAN) == EXPECTED_PLAN
assert digest_file(PROVENANCE) == EXPECTED_PROVENANCE
assert digest_file(STAGE_RECORD) == EXPECTED_STAGE_RECORD
assert digest_file(PREFLIGHT_RESULT) == EXPECTED_PREFLIGHT
assert digest_file(LOG) == EXPECTED_LOG
assert digest_file(DELTA_MANIFEST) == EXPECTED_DELTA_MANIFEST
assert not os.path.lexists(RESULT)

base_spec = importlib.util.spec_from_file_location('reviewed_base_materializer_verify', BASE_MATERIALIZER)
base = importlib.util.module_from_spec(base_spec)
base_spec.loader.exec_module(base)
delta_spec = importlib.util.spec_from_file_location('reviewed_input_delta_verify', DELTA_SCRIPT)
delta = importlib.util.module_from_spec(delta_spec)
delta_spec.loader.exec_module(delta)
manifest = json.loads(DELTA_MANIFEST.read_bytes())
assert manifest['base_rootfs_tree_sha256'] == EXPECTED_BASE_TREE
assert manifest['result_rootfs_tree_sha256'] == EXPECTED_RESULT_TREE
assert manifest['plan_sha256'] == EXPECTED_PLAN
assert manifest['tool_provenance_sha256'] == EXPECTED_PROVENANCE
assert manifest['added_path'] == '/input'
assert manifest['base_manifest_sha256'] == EXPECTED_BASE_MANIFEST
assert manifest['added_record'] == {'gid': 0, 'mode': 0o755, 'path': '/input', 'type': 'directory', 'uid': 0}

tree = base.tree_hash(str(ROOTFS))
task_fd = delta.open_task_dir()
try:
    root_fd = os.open('rootfs', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        records = delta.tree_records(root_fd)
        tree_again = delta.digest_records(records)
        input_fd = os.open('input', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=root_fd)
        try:
            input_st = os.fstat(input_fd)
            assert stat.S_IMODE(input_st.st_mode) == 0o755 and (input_st.st_uid, input_st.st_gid) == (0, 0)
            assert os.listdir(input_fd) == []
        finally:
            os.close(input_fd)
    finally:
        os.close(root_fd)
finally:
    os.close(task_fd)
assert tree == EXPECTED_RESULT_TREE and tree_again == EXPECTED_RESULT_TREE

record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'delta_script_sha256': EXPECTED_DELTA,
    'delta_manifest_sha256': EXPECTED_DELTA_MANIFEST,
    'delta_log_sha256': EXPECTED_LOG,
    'preflight_result_sha256': EXPECTED_PREFLIGHT,
    'base_manifest_sha256': EXPECTED_BASE_MANIFEST,
    'base_tree_sha256': EXPECTED_BASE_TREE,
    'plan_sha256': EXPECTED_PLAN,
    'provenance_sha256': EXPECTED_PROVENANCE,
    'result_tree_sha256': tree,
    'checks': {
        'reviewed_base_hasher_matches_result': True,
        'independent_delta_hasher_matches_result': True,
        'input_is_empty_root_owned_mode0755_directory': True,
    },
    'scope': 'read-only verification of the single task-owned /input rootfs delta; no tools, network, mounts, Cargo, helper, or generator executed',
}
with RESULT.open('x', encoding='utf-8') as output:
    json.dump(record, output, sort_keys=True, separators=(',', ':'))
    output.write('\n')
os.chmod(RESULT, 0o600)
print('PASS: rootfs tree matches the reviewed /input delta manifest; tree=' + tree)
