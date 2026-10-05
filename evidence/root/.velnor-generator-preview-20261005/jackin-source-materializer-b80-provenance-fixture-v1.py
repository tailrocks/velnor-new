from pathlib import Path
import copy
import hashlib
import importlib.util
import json

TASK = Path('/root/.velnor-generator-preview-20261005')
MATERIALIZER = TASK / 'jackin-source-materialize-b80-v2.py'
MATERIALIZER_SHA256 = '5ff2be8985c0620d82a487375c9358ea1d34569eb36903a1ec3578a609e5c541'
actual = hashlib.sha256(MATERIALIZER.read_bytes()).hexdigest()
if actual != MATERIALIZER_SHA256:
    raise SystemExit('materializer hash changed before fixture execution')
spec = importlib.util.spec_from_file_location('jackin_materializer', MATERIALIZER)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
profile = json.loads((TASK / 'jackin-source-profile-b80f991d56c2.json').read_bytes())
transcript = json.loads((TASK / 'jackin-source-b80f991d56c2-exact-v2-fetch.json').read_bytes())
record = module.source_provenance_record(profile, transcript)
if record['branch_tip_at_selection'] != module.COMMIT:
    raise SystemExit('selection-time branch tip was not recorded')
if record['fetched_branch_tip'] == module.COMMIT:
    raise SystemExit('fixture failed to preserve observed branch movement')
if record['signature_verified'] is not False:
    raise SystemExit('fixture failed to preserve unsigned-profile disposition')
if record['credential_access'] != transcript['credential_access']:
    raise SystemExit('fixture failed to preserve fetch credential scope')
manifest = {
    'schema': 1,
    'repository': module.ORIGIN,
    'commit': module.COMMIT,
    'tree': module.TREE,
    'branch': module.BRANCH,
    'parent': module.PARENT,
    'profile_sha256': module.PROFILE_SHA256,
    'fetch_transcript_sha256': module.TRANSCRIPT_SHA256,
    'source_provenance': record,
}
module.validate_manifest_provenance(manifest, record)
checks = 0

def reject(label, action):
    global checks
    try:
        action()
    except SystemExit:
        checks += 1
    else:
        raise SystemExit('expected fail-closed rejection: ' + label)

for field, value in (
    ('branch_tip_at_selection', module.PARENT),
    ('signature_verified', True),
):
    altered = copy.deepcopy(profile)
    altered[field] = value
    reject('profile ' + field, lambda altered=altered: module.source_provenance_record(altered, transcript))
for field, value in (
    ('advertised_tip_before_fetch', module.PARENT),
    ('fetched_branch_tip', module.PARENT),
    ('advertised_tip_after_fetch', module.PARENT),
    ('tip_stable_during_fetch', False),
    ('credential_access', 'unknown'),
    ('source_policy', 'latest branch only'),
):
    altered = copy.deepcopy(transcript)
    altered[field] = value
    reject('transcript ' + field, lambda altered=altered: module.source_provenance_record(profile, altered))
for field, value in (
    ('schema', 2),
    ('repository', 'https://example.invalid/attacker.git'),
    ('commit', module.PARENT),
    ('tree', module.PARENT),
    ('branch', 'main'),
    ('parent', module.COMMIT),
    ('profile_sha256', '0' * 64),
    ('fetch_transcript_sha256', '1' * 64),
    ('source_provenance', {}),
):
    altered = copy.deepcopy(manifest)
    altered[field] = value
    reject('manifest ' + field, lambda altered=altered: module.validate_manifest_provenance(altered, record))
if checks != 17:
    raise SystemExit('unexpected mutation coverage: ' + str(checks))
print('PASS: 17 provenance mutations rejected; exact profile/transcript and manifest accepted')
