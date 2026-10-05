import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import stat
import sys

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
DELTA_PATH = TASK / 'preseed-preview-rootfs-input-delta.py'
BASE_MATERIALIZER = TASK / 'preseed-preview-materialize.py'
PLAN_OLD = TASK / 'rootfs-stage-plan.json'
PLAN_NEW = TASK / 'rootfs-stage-plan-with-input.json'
BASE_MANIFEST = TASK / 'rootfs-base-manifest.json'
PROVENANCE = TASK / 'official-tool-asset-provenance.json'
STAGE_RECORD = TASK / 'base-stage-attempt-3.json'
ROOTFS = TASK / 'rootfs'
RESULT = TASK / 'rootfs-input-delta-preflight-result.json'
DELTA_SHA = '40e2acf6f621888e3a6f645dc86755d4f5e338d22b5ae9bba53b4f037967377a'
BASE_MATERIALIZER_SHA = '4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3'
BASE_TREE = '1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763'
OLD_PLAN_SHA = 'ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f'
NEW_PLAN_SHA = '4c4a18dd85d71aaa9c9fc9352da9553fc2eb7f5c2d10a8365058c19ea04597bc'
BASE_MANIFEST_SHA = '1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf'
PROVENANCE_SHA = '67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137'
STAGE_RECORD_SHA = '89b9feb8b946c8d435e3978395db69b632f23769e18da41fcfc5e57b9afb37e4'

assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site

def checked_digest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        assert stat.S_ISREG(st.st_mode) and (st.st_uid, st.st_gid) == (0, 0) and st.st_nlink == 1
        hasher = hashlib.sha256()
        while True:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            hasher.update(chunk)
        return hasher.hexdigest()
    finally:
        os.close(fd)

assert checked_digest(DELTA_PATH) == DELTA_SHA
assert checked_digest(BASE_MATERIALIZER) == BASE_MATERIALIZER_SHA
assert checked_digest(PLAN_OLD) == OLD_PLAN_SHA
assert checked_digest(PLAN_NEW) == NEW_PLAN_SHA
assert checked_digest(BASE_MANIFEST) == BASE_MANIFEST_SHA
assert checked_digest(PROVENANCE) == PROVENANCE_SHA
assert checked_digest(STAGE_RECORD) == STAGE_RECORD_SHA
stage_record = json.loads(STAGE_RECORD.read_bytes())
assert stage_record['status'] == 'PASS' and stage_record['exit_code'] == 0
assert stage_record['base_manifest_sha256'] == BASE_MANIFEST_SHA
assert stage_record['plan_sha256'] == OLD_PLAN_SHA
assert stage_record['tool_provenance_sha256'] == PROVENANCE_SHA
assert stage_record['rootfs_tree_sha256'] == BASE_TREE

delta_spec = importlib.util.spec_from_file_location('reviewed_rootfs_input_delta', DELTA_PATH)
delta = importlib.util.module_from_spec(delta_spec)
delta_spec.loader.exec_module(delta)
base_spec = importlib.util.spec_from_file_location('reviewed_base_materializer', BASE_MATERIALIZER)
base = importlib.util.module_from_spec(base_spec)
base_spec.loader.exec_module(base)

task_fd = delta.open_task_dir()
try:
    root_fd = os.open('rootfs', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        records = delta.tree_records(root_fd)
        actual_tree = delta.digest_records(records)
        assert actual_tree == BASE_TREE
        assert base.tree_hash(str(ROOTFS)) == BASE_TREE
        assert not any(record['path'] == '/input' for record in records)
        try:
            os.stat('input', dir_fd=root_fd, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise AssertionError('/input unexpectedly exists')
    finally:
        os.close(root_fd)
finally:
    os.close(task_fd)

old_plan = json.loads(PLAN_OLD.read_bytes())
new_plan = json.loads(PLAN_NEW.read_bytes())
assert new_plan['rootfs_dirs'].count({'mode': '0755', 'owner': 'root:root', 'path': '/input'}) == 1
new_without_input = dict(new_plan)
new_without_input['rootfs_dirs'] = [item for item in new_plan['rootfs_dirs'] if item['path'] != '/input']
assert new_without_input == old_plan
expected_after = list(records)
insert_at = next(index for index, record in enumerate(expected_after) if record['path'] == '/lib')
new_input = {'path': '/input', 'type': 'directory', 'mode': 0o755, 'uid': 0, 'gid': 0}
expected_after.insert(insert_at, new_input)
expected_after_tree = delta.digest_records(expected_after)

assert not (TASK / 'rootfs-input-delta-manifest.json').exists()
assert not (TASK / 'rootfs-input-delta.log').exists()
record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'delta_script_sha256': DELTA_SHA,
    'old_rootfs_tree_sha256': actual_tree,
    'expected_new_rootfs_tree_sha256': expected_after_tree,
    'old_plan_sha256': OLD_PLAN_SHA,
    'new_plan_sha256': NEW_PLAN_SHA,
    'base_manifest_sha256': BASE_MANIFEST_SHA,
    'provenance_sha256': PROVENANCE_SHA,
    'stage_record_sha256': STAGE_RECORD_SHA,
    'checks': {
        'old_tree_matches_base_manifest_and_independent_hasher': True,
        'source_stage_record_and_provenance_hashes_match_embedded_pins': True,
        'input_path_absent': True,
        'new_plan_adds_only_empty_input_directory': True,
        'expected_result_tree_is_old_records_plus_one_input_record': True,
        'delta_manifest_and_stage_log_absent': True,
    },
    'scope': 'read-only rootfs delta preflight; no rootfs writes, tool execution, network, mounts, or Cargo',
}
with RESULT.open('x', encoding='utf-8') as output:
    json.dump(record, output, sort_keys=True, separators=(',', ':'))
    output.write('\n')
os.chmod(RESULT, 0o600)
print('PASS: rootfs base and single /input delta preflight; old_tree=' + actual_tree + ' new_tree=' + expected_after_tree)
