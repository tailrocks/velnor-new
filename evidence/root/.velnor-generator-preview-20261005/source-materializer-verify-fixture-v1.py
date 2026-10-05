import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import stat
import sys
import tempfile

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
HELPER = TASK / 'preseed-preview-source-materialize.py'
SOURCE = TASK / 'main-source-ac3ab'
MANIFEST = TASK / 'main-source-ac3ab-manifest.json'
RESULT = TASK / 'source-materializer-final-verification-result.json'
EXPECTED_HELPER = '9cd0e15839c22af33fbb14b91db1234bdb7dda15b1cf0c7b65412c19df0515fd'
EXPECTED_MANIFEST = '6c56696f0e5bdaa0ab1462139c614835353d43f2dc3da2f4a45beeec1a017ef3'
EXPECTED_INDEX = '2511edb3f26b2b66020e8f938dd2a23971d80a257b9dc369d1cefcc853d187e9'
assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site
helper_bytes = HELPER.read_bytes()
helper_sha = hashlib.sha256(helper_bytes).hexdigest()
assert helper_sha == EXPECTED_HELPER

def sha256_path(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()

manifest_path = SOURCE / '.git' / 'index'
index_path = SOURCE / '.git' / 'index'
assert sha256_path(MANIFEST) == EXPECTED_MANIFEST
assert sha256_path(index_path) == EXPECTED_INDEX
before = {'manifest': sha256_path(MANIFEST), 'index': sha256_path(index_path)}

spec = importlib.util.spec_from_file_location('reviewed_source_materializer_verify', HELPER)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
boundary_task = pathlib.Path(tempfile.mkdtemp(prefix='source-materializer-verify-boundary-', dir=TASK))
os.chmod(boundary_task, 0o700)
module.TASK = str(boundary_task)
sys.argv = ['source-materializer-verify-fixture-v1.py', '--verify']
module.early_boundary()
module.verify_materialized()

after = {'manifest': sha256_path(MANIFEST), 'index': sha256_path(index_path)}
assert before == after
assert after == {'manifest': EXPECTED_MANIFEST, 'index': EXPECTED_INDEX}
for path in (MANIFEST, index_path):
    st = path.stat(follow_symlinks=False)
    assert stat.S_ISREG(st.st_mode) and (st.st_uid, st.st_gid) == (0, 0)
    assert st.st_nlink == 1 and not (stat.S_IMODE(st.st_mode) & 0o022)

record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'materializer_sha256': helper_sha,
    'source_commit': module.COMMIT,
    'source_tree': module.TREE,
    'manifest_sha256': after['manifest'],
    'index_sha256': after['index'],
    'checks': {
        'complete_manifest_index_and_worktree_verification': True,
        'manifest_and_index_unchanged': True,
        'root_owner_single_link_and_nonwritable_group_other': True,
    },
    'scope': 'read-only final source verification under the reviewed isolated boundary; no generator, Cargo, helper binary, credentials, image, or container operation',
}
with RESULT.open('x', encoding='utf-8') as output:
    json.dump(record, output, sort_keys=True, separators=(',', ':'))
    output.write('\n')
os.chmod(RESULT, 0o600)
