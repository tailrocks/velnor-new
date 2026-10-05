import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import stat
import sys
import tempfile

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
HELPER = TASK / 'preseed-preview-source-materialize.py'
SOURCE = TASK / 'main-source-ac3ab'
EXPECTED = '9cd0e15839c22af33fbb14b91db1234bdb7dda15b1cf0c7b65412c19df0515fd'
FIXTURE = TASK / 'source-materializer-fixture-result-v7.json'
assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site
helper_bytes = HELPER.read_bytes()
helper_sha = hashlib.sha256(helper_bytes).hexdigest()
assert helper_sha == EXPECTED, (helper_sha, EXPECTED)
spec = importlib.util.spec_from_file_location('reviewed_source_materializer_v7', HELPER)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
boundary_task = pathlib.Path(tempfile.mkdtemp(prefix='source-materializer-boundary-v7-', dir=TASK))
os.chmod(boundary_task, 0o700)
module.TASK = str(boundary_task)
sys.argv = ['source-materializer-fixture-v7.py', '--verify-partial']
module.early_boundary()

# Verify the complete partially materialized tree from the pinned Git objects.
entries, gitlinks, lfs_pointers = module.verify_partial_tree()
assert len(entries) > 1000
assert len(gitlinks) == 1
assert lfs_pointers == [{
    'path': 'qualification/fixtures/lfs-marker.txt',
    'git_blob_oid': '2127683de39536188b5e2d719e96c2fade5f2d61',
    'lfs_oid': '8c271ba1c7127e3409055b635a6a2e2671c004ff9f4a507c2cdfd9e0fb3bf90b',
    'lfs_size': 7,
    'pointer_kept': True,
}]
assert not (SOURCE / '.git' / 'index').exists()
assert not (TASK / 'main-source-ac3ab-manifest.json').exists()

scratch = pathlib.Path(tempfile.mkdtemp(prefix='source-materializer-fixture-v7-', dir=TASK))
os.chmod(scratch, 0o700)
module.TASK = str(scratch)
module.SOURCE = str(scratch / 'synthetic-source')
module.MANIFEST = str(scratch / 'synthetic-manifest.json')
module.AUDIT = str(TASK / 'preseed-preview-git-config-audit.py')
worktree = scratch / 'synthetic-worktree'
worktree.mkdir(mode=0o700)

def blob_entry(path, data, mode='100644'):
    oid = hashlib.sha1(b'blob ' + str(len(data)).encode('ascii') + b'\0' + data).hexdigest()
    return {'path': path, 'parts': path.split('/'), 'mode': mode, 'type': 'blob', 'oid': oid}, oid

pointer = (
    b'version https://git-lfs.github.com/spec/v1\n'
    + b'oid sha256:' + b'a1' * 32 + b'\n'
    + b'size 4096\n'
)
synthetic_entries = []
synthetic_objects = {}
for path, data in [
    ('group/a-lfs-pointer.txt', pointer),
    ('group/b-sibling-one.txt', b'first sibling\n'),
    ('group/c-sibling-two.txt', b'second sibling\n'),
    ('group/nested/leaf.txt', b'nested after siblings\n'),
    ('root-file.txt', b'root file after group\n'),
]:
    item, oid = blob_entry(path, data)
    synthetic_entries.append(item)
    synthetic_objects[oid] = data
symlink, symlink_oid = blob_entry('links/safe', b'../group/b-sibling-one.txt', '120000')
synthetic_entries.append(symlink)
synthetic_objects[symlink_oid] = b'../group/b-sibling-one.txt'
synthetic_entries.append({
    'path': 'qualification/fixtures/submodule',
    'parts': ['qualification', 'fixtures', 'submodule'],
    'mode': '160000', 'type': 'gitlink', 'oid': '85e202a9446b105a9c8b8e9589b54bc1e513a808',
})
synthetic_entries.sort(key=lambda item: item['path'].encode('utf-8'))
for item in synthetic_entries:
    if item['mode'] == '120000':
        item['symlink_target'] = module.safe_symlink(item['path'], synthetic_objects[item['oid']])

root_fd = os.open(worktree, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
try:
    synthetic_gitlinks, synthetic_lfs = module.materialize(root_fd, synthetic_entries, synthetic_objects)
    checked_lfs = module.verify_worktree(root_fd, synthetic_entries)
finally:
    os.close(root_fd)
assert synthetic_gitlinks == [{
    'path': 'qualification/fixtures/submodule',
    'commit': '85e202a9446b105a9c8b8e9589b54bc1e513a808',
    'content_materialized': False,
}]
expected_synthetic_lfs = [{
    'path': 'group/a-lfs-pointer.txt',
    'git_blob_oid': synthetic_entries[0]['oid'],
    'lfs_oid': 'a1' * 32,
    'lfs_size': 4096,
    'pointer_kept': True,
}]
assert synthetic_lfs == expected_synthetic_lfs
assert checked_lfs == expected_synthetic_lfs
assert (worktree / 'links' / 'safe').is_symlink()
assert os.readlink(worktree / 'links' / 'safe') == '../group/b-sibling-one.txt'
assert stat.S_IMODE((worktree / 'group').stat().st_mode) == 0o755
assert stat.S_IMODE((worktree / 'group' / 'nested').stat().st_mode) == 0o755
assert stat.S_IMODE((worktree / 'qualification' / 'fixtures' / 'submodule').stat().st_mode) == 0o755

unsafe_root = scratch / 'unsafe-root'
unsafe_root.mkdir(mode=0o700)
unsafe_tree = unsafe_root / 'unsafe-existing-directory'
unsafe_tree.mkdir(mode=0o700)
unsafe_fd = os.open(unsafe_root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
unsafe_entry, unsafe_oid = blob_entry('unsafe-existing-directory/child', b'must not be written\n')
try:
    try:
        module.materialize(unsafe_fd, [unsafe_entry], {unsafe_oid: b'must not be written\n'})
    except SystemExit:
        pass
    else:
        raise AssertionError('unsafe existing directory was accepted')
finally:
    os.close(unsafe_fd)
assert stat.S_IMODE(unsafe_tree.stat().st_mode) == 0o700
assert list(unsafe_tree.iterdir()) == []

record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'materializer_sha256': helper_sha,
    'source_commit': module.COMMIT,
    'source_tree': module.TREE,
    'authenticated_partial_entries': len(entries),
    'authenticated_partial_gitlinks': len(gitlinks),
    'authenticated_partial_lfs_pointers': lfs_pointers,
    'checks': {
        'full_early_boundary_under_umask_077': True,
        'full_authenticated_partial_tree_hash_and_path_verification': True,
        'no_index_or_manifest_was_created_by_partial_verification': True,
        'multiple_sibling_files_and_following_directory': True,
        'valid_git_lfs_pointer_inventory': True,
        'gitlink_remains_empty_and_unmaterialized': True,
        'safe_relative_symlink_preserved': True,
        'unsafe_existing_directory_rejected_without_chmod_or_write': True,
    },
    'scope': 'read-only qualification of authenticated partial tree plus isolated synthetic path fixtures; no source finalization, helper, generator, Cargo, credential, image, or container operation',
}
with FIXTURE.open('x', encoding='utf-8') as output:
    json.dump(record, output, sort_keys=True, separators=(',', ':'))
    output.write('\n')
os.chmod(FIXTURE, 0o600)
fixture_sha = hashlib.sha256(FIXTURE.read_bytes()).hexdigest()
print('PASS: full partial-source qualification and sibling/LFS fixtures; materializer=' + helper_sha + ' result=' + fixture_sha)
shutil.rmtree(scratch)
