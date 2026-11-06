# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import zlib
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

REPO = 'tailrocks/velnor-new'
WORKFLOW = '.github/workflows/binary-preview.yml'
ISSUER = 'https://token.actions.githubusercontent.com'
PREDICATE = 'https://slsa.dev/provenance/v1'
TARGET = 'x86_64-unknown-linux-gnu'
MAX_API = 8 * 1024 * 1024
MAX_ARCHIVE = 256 * 1024 * 1024


def require(condition, name):
    if not condition:
        raise ValueError('sdk_preview_bootstrap:' + name)


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, 'duplicate_json_key')
        result[key] = value
    return result


def parse(raw):
    require(len(raw) <= MAX_API, 'json_limit')
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=lambda _: require(False, 'nonfinite_json'))


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def closed(value, keys):
    require(type(value) is dict and set(value) == set(keys.split()), 'closed_schema')


def positive(value):
    return type(value) is int and value > 0


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def command(arguments, limit=MAX_API, timeout=120):
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as error:
        process = subprocess.Popen(arguments, stdout=output, stderr=error)
        started = time.monotonic()
        while process.poll() is None:
            if os.fstat(output.fileno()).st_size > limit or os.fstat(error.fileno()).st_size > MAX_API or time.monotonic() - started > timeout:
                process.kill()
                process.wait()
                raise ValueError('sdk_preview_bootstrap:command_limit')
            time.sleep(0.1)
        require(process.returncode == 0, 'command_failed')
        require(os.fstat(output.fileno()).st_size <= limit, 'command_limit')
        output.seek(0)
        return output.read(limit + 1)


def api(endpoint):
    return parse(command([GH, 'api', endpoint]))


def pages(endpoint):
    result = parse(command([GH, 'api', '--paginate', '--slurp', endpoint]))
    require(type(result) is list and 0 < len(result) <= 1024, 'pagination')
    return result


def assets(release_id):
    rows = [row for page in pages(f'repos/{REPO}/releases/{release_id}/assets?per_page=100') for row in page]
    require(0 < len(rows) <= 32, 'asset_count')
    result = {}
    ids = set()
    for row in rows:
        name = row['name']
        require(type(name) is str and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,254}', name), 'asset_name')
        require(name not in result and positive(row['id']) and row['id'] not in ids and positive(row['size']) and row['size'] <= MAX_ARCHIVE and row['state'] == 'uploaded', 'asset_metadata')
        ids.add(row['id'])
        result[name] = row
    return result


def download(row):
    raw = command([GH, 'api', '-H', 'Accept: application/octet-stream', f'repos/{REPO}/releases/assets/{row["id"]}'], row['size'])
    require(len(raw) == row['size'], 'asset_size')
    if row.get('digest') is not None:
        require(row['digest'] == 'sha256:' + digest(raw), 'asset_api_digest')
    return raw


def ancestry(base, head):
    comparison = api(f'repos/{REPO}/compare/{base}...{head}')
    require(comparison['status'] in ('ahead', 'identical') and comparison['base_commit']['sha'] == base and comparison['merge_base_commit']['sha'] == base, 'ancestry')


def verify_run(run, run_id, attempt, source, path, event):
    require(run['id'] == run_id and run['run_attempt'] == attempt and run['head_sha'] == source and run['path'] == path and run['event'] == event and run['status'] == 'completed' and run['conclusion'] == 'success' and run['head_branch'] == 'main', 'run_identity')
    for key in ('repository', 'head_repository'):
        require(run[key]['id'] == REPOSITORY_ID and run[key]['full_name'] == REPO, 'run_repository')
    workflow = api(f'repos/{REPO}/actions/workflows/{run["workflow_id"]}')
    require(workflow['id'] == run['workflow_id'] and workflow['path'] == path and workflow['state'] == 'active', 'workflow_identity')


def latest_source_ci(workflow_id):
    runs = [run for page in pages(f'repos/{REPO}/actions/workflows/{workflow_id}/runs?head_sha={SOURCE}&per_page=100') for run in page['workflow_runs'] if run['event'] == 'push']
    require(runs and len({run['id'] for run in runs}) == len(runs) and len({run['run_number'] for run in runs}) == len(runs), 'ci_inventory')
    for run in runs:
        require(positive(run['id']) and positive(run['run_number']) and positive(run['run_attempt']) and run['head_sha'] == SOURCE and run['head_branch'] == 'main' and run['workflow_id'] == workflow_id and run['path'] == '.github/workflows/ci.yml', 'ci_inventory_identity')
        for key in ('repository','head_repository'):
            require(run[key]['id'] == REPOSITORY_ID and run[key]['full_name'] == REPO, 'ci_inventory_repository')
    return max(runs,key=lambda run:run['run_number'])


def receipt(raw, expected_source):
    value = parse(raw)
    require(canonical(value) == raw, 'receipt_canonical')
    closed(value, 'schema channel repository source workflow version tag products companions assets evidence')
    require(value['schema'] == 'velnor.package-release.v2' and value['channel'] == 'preview' and value['tag'] == 'preview-' + expected_source, 'receipt_identity')
    closed(value['repository'], 'slug id')
    require(positive(value['repository']['id']) and value['repository'] == {'slug':REPO, 'id':REPOSITORY_ID}, 'receipt_repository')
    source = value['source']
    closed(source, 'commit_sha ref default_branch event ci_run_id ci_run_attempt')
    require(source['commit_sha'] == expected_source and source['ref'] == 'refs/heads/main' and source['default_branch'] == 'main' and source['event'] == 'workflow_run' and positive(source['ci_run_id']) and positive(source['ci_run_attempt']), 'receipt_source')
    workflow = value['workflow']
    closed(workflow, 'path ref authority_sha run_id run_attempt producer_run_id producer_run_attempt signer_job_id signer_workflow')
    require(workflow['path'] == WORKFLOW and workflow['ref'] == 'refs/heads/main' and re.fullmatch('[a-f0-9]{40}', workflow['authority_sha']) and positive(workflow['run_id']) and positive(workflow['run_attempt']) and positive(workflow['producer_run_id']) and positive(workflow['producer_run_attempt']) and workflow['producer_run_id'] == workflow['run_id'] and workflow['producer_run_attempt'] == workflow['run_attempt'] and workflow['signer_job_id'] == 'preview-signer' and workflow['signer_workflow'] == WORKFLOW, 'receipt_workflow')
    version = value['version']
    closed(version, 'base value commit_count source_sha7')
    require(version['base'] == BASE_VERSION and positive(version['commit_count']) and version['source_sha7'] == expected_source[:7] and version['value'] == f'{BASE_VERSION}-preview.{version["commit_count"]}+{expected_source[:7]}', 'receipt_version')
    require(value['companions'] == [] and type(value['products']) is list and len(value['products']) == 2, 'receipt_products')
    bindings = {}
    for product, (name, package) in zip(value['products'], [('velnor-actions','velnor-actions-cli'), ('velnor-archive-guard','velnor-archive-guard')]):
        closed(product, 'id package package_version bins payloads')
        archive = name + '-' + TARGET + '.tar.gz'
        require(product == {'id':name,'package':package,'package_version':BASE_VERSION,'bins':[name],'payloads':[{'format':'tar-gz','target':TARGET,'name':archive,'members':[name]}]}, 'receipt_product_inventory')
        for suffix, kind in [('', 'payload'), ('.bundle','signature_bundle'), ('.sha256','sha256'), ('.sbom.json','sbom')]:
            bindings[archive + suffix] = (kind, name, TARGET, archive)
    bindings['SHA256SUMS'] = ('checksum_list', None, None, None)
    require(type(value['assets']) is list and [a['name'] for a in value['assets']] == sorted(bindings), 'receipt_asset_inventory')
    for asset in value['assets']:
        closed(asset, 'name kind product_id target subject_name size sha256')
        require((asset['kind'], asset['product_id'], asset['target'], asset['subject_name']) == bindings[asset['name']] and positive(asset['size']) and asset['size'] <= MAX_ARCHIVE and re.fullmatch('[a-f0-9]{64}', asset['sha256']), 'receipt_asset_binding')
    evidence = value['evidence']
    closed(evidence, 'profile sbom_format signer_issuer signer_workflow_ref attestation_predicate attested_subject_names')
    require(evidence == {'profile':'signed-sbom-v1','sbom_format':'CycloneDX-JSON','signer_issuer':ISSUER,'signer_workflow_ref':f'{REPO}/{WORKFLOW}@refs/heads/main','attestation_predicate':PREDICATE,'attested_subject_names': sorted(['release.json'] + [a['name'] for a in value['assets'] if a['kind'] == 'payload'])}, 'receipt_evidence')
    return value


def decode_snappy(raw):
    require(0 < len(raw) <= MAX_API, 'snappy_input_limit')
    size = 0
    position = 0
    for shift in range(0,35,7):
        require(position < len(raw), 'snappy_length_truncated')
        byte = raw[position]
        position += 1
        require(shift != 28 or byte < 16, 'snappy_length_overflow')
        size |= (byte & 127) << shift
        if byte < 128:
            break
    else:
        raise ValueError('sdk_preview_bootstrap:snappy_length_overflow')
    require(0 < size <= MAX_API, 'snappy_expansion_limit')
    output = bytearray()
    while position < len(raw):
        tag = raw[position]
        position += 1
        kind = tag & 3
        if kind == 0:
            length = tag >> 2
            if length < 60:
                length += 1
            else:
                count = length - 59
                require(position + count <= len(raw), 'snappy_literal_length')
                length = int.from_bytes(raw[position:position+count],'little') + 1
                position += count
            require(position + length <= len(raw) and len(output) + length <= size, 'snappy_literal_limit')
            output.extend(raw[position:position+length])
            position += length
        else:
            count = [0,1,2,4][kind]
            require(position + count <= len(raw), 'snappy_copy_truncated')
            offset = int.from_bytes(raw[position:position+count],'little')
            position += count
            if kind == 1:
                offset |= (tag & 224) << 3
                length = 4 + ((tag >> 2) & 7)
            else:
                length = 1 + (tag >> 2)
            require(0 < offset <= len(output) and len(output) + length <= size, 'snappy_copy_limit')
            for _ in range(length):
                output.append(output[-offset])
    require(len(output) == size, 'snappy_output_size')
    return bytes(output)


def validate_bundle_url(value):
    url = urllib.parse.urlsplit(value)
    require(url.scheme == 'https' and url.hostname and url.username is None and url.password is None and url.port in (None,443) and (url.hostname.endswith('.blob.core.windows.net') or url.hostname.endswith('.actions.githubusercontent.com')), 'attestation_bundle_url')


class BundleRedirect(urllib.request.HTTPRedirectHandler):
    max_repeats = 5
    max_redirections = 5

    def redirect_request(self, request, file, code, message, headers, new_url):
        validate_bundle_url(new_url)
        return super().redirect_request(request,file,code,message,headers,new_url)


def verify_attestation(name, raw, all_subjects, value, directory):
    bundles = []
    for page in pages(f'repos/{REPO}/attestations/sha256:{digest(raw)}?per_page=100&predicate_type=https%3A%2F%2Fslsa.dev%2Fprovenance%2Fv1'):
        for item in page['attestations']:
            bundle = item.get('bundle')
            if bundle is None:
                validate_bundle_url(item['bundle_url'])
                request = urllib.request.Request(item['bundle_url'],headers={'Accept-Encoding':'identity'})
                with urllib.request.build_opener(BundleRedirect()).open(request, timeout=60) as response:
                    require(response.status == 200 and response.headers.get('Content-Encoding','identity') == 'identity', 'attestation_bundle_encoding')
                    bundle = parse(decode_snappy(response.read(MAX_API + 1)))
            encoded = canonical(bundle)
            require(encoded not in bundles and sum(map(len,bundles)) + len(encoded) < MAX_API, 'attestation_bundle_inventory')
            bundles.append(encoded)
    require(bundles, 'attestation_missing')
    bundle_path = directory / (name + '.attestations.jsonl')
    bundle_path.write_bytes(b'\n'.join(bundles) + b'\n')
    verified = parse(command([GH,'attestation','verify',str(directory/name),'--bundle',str(bundle_path),'--repo',REPO,'--predicate-type',PREDICATE,'--cert-oidc-issuer',ISSUER,'--format','json']))
    workflow = value['workflow']
    uri = f'https://github.com/{REPO}/{WORKFLOW}@refs/heads/main'
    expected = {'issuer':ISSUER,'subjectAlternativeName':uri,'sourceRepositoryURI':f'https://github.com/{REPO}','sourceRepositoryOwnerURI':'https://github.com/tailrocks','sourceRepositoryIdentifier':str(REPOSITORY_ID),'sourceRepositoryDigest':SOURCE,'sourceRepositoryRef':'refs/heads/main','buildSignerURI':uri,'buildSignerDigest':workflow['authority_sha'],'buildConfigURI':uri,'buildConfigDigest':workflow['authority_sha'],'buildTrigger':'workflow_run','runnerEnvironment':'github-hosted','runInvocationURI':f'https://github.com/{REPO}/actions/runs/{workflow["run_id"]}/attempts/{workflow["run_attempt"]}'}
    matched = []
    for attestation in verified:
        result = attestation['verificationResult']
        certificate = result['signature']['certificate']
        if certificate.get('runInvocationURI') != expected['runInvocationURI']:
            continue
        require(all(certificate.get(k) == v for k,v in expected.items()), 'attestation_certificate')
        statement = result['statement']
        require(statement['predicateType'] == PREDICATE and statement['predicate']['buildDefinition']['internalParameters']['github']['event_name'] == 'workflow_run', 'attestation_predicate')
        subjects = statement['subject']
        require(len(subjects) == len(all_subjects) and {s['name']:s['digest'] for s in subjects} == {n:{'sha256':d} for n,d in all_subjects.items()}, 'attestation_subjects')
        matched.append(attestation)
    require(len(matched) == 1, 'attestation_run')


def extract(raw, name):
    stream = zlib.decompressobj(31)
    unpacked = stream.decompress(raw, MAX_ARCHIVE + 1)
    require(len(unpacked) <= MAX_ARCHIVE and stream.eof and not stream.unused_data and not stream.unconsumed_tail, 'archive_expansion_limit')
    archive = tarfile.open(fileobj=io.BytesIO(unpacked), mode='r:')
    members = archive.getmembers()
    require(len(members) == 1, 'archive_inventory')
    member = members[0]
    require(member.offset == 0 and member.offset_data == 512 and member.name == name and member.isfile() and member.type == tarfile.REGTYPE and not member.pax_headers and member.linkname == '' and 0 < member.size <= MAX_ARCHIVE and member.mode & 0o111, 'archive_member')
    require(len(unpacked) >= member.offset_data + member.size + 1024 and all(byte == 0 for byte in unpacked[member.offset_data + member.size:]), 'archive_trailing')
    binary = archive.extractfile(member).read(MAX_ARCHIVE + 1)
    require(len(binary) == member.size and binary[:6] == b'\x7fELF\x02\x01' and binary[18:20] == b'\x3e\x00', 'archive_executable')
    return binary


def main():
    global GH, SOURCE, REPOSITORY_ID, BASE_VERSION
    BASE_VERSION = os.environ['VELNOR_SDK_BASE_VERSION']
    require(re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', BASE_VERSION), 'base_version')
    GH = os.environ['VELNOR_BOOTSTRAP_GH']
    SOURCE = os.environ['VELNOR_IMAGE_SOURCE_SHA']
    require(re.fullmatch('[a-f0-9]{40}', SOURCE), 'source_sha')
    repository = api('repos/' + REPO)
    require(repository['full_name'] == REPO and repository['default_branch'] == 'main' and positive(repository['id']), 'repository')
    REPOSITORY_ID = repository['id']
    release = None
    for _ in range(120):
        listing = [item for page in pages(f'repos/{REPO}/releases?per_page=100') for item in page if item['tag_name'] == 'preview-' + SOURCE]
        require(len(listing) <= 1, 'release_ambiguous')
        if listing and not listing[0]['draft']:
            release = listing[0]
            break
        time.sleep(10)
    require(release is not None and release['immutable'] is True and release['prerelease'] is True and release['author']['id'] == 41898282 and release['author']['login'] == 'github-actions[bot]', 'immutable_release')
    listing = assets(release['id'])
    require('release.json' in listing and 'release.json.bundle' in listing, 'receipt_missing')
    raw = download(listing['release.json'])
    value = receipt(raw, SOURCE)
    require(set(listing) == {a['name'] for a in value['assets']} | {'release.json','release.json.bundle'}, 'public_asset_inventory')
    tag = api(f'repos/{REPO}/git/ref/tags/preview-{SOURCE}')
    require(tag['ref'] == 'refs/tags/preview-' + SOURCE and tag['object']['type'] == 'tag' and re.fullmatch('[a-f0-9]{40}',tag['object']['sha']), 'annotated_tag_reference')
    annotated = api(f'repos/{REPO}/git/tags/{tag["object"]["sha"]}')
    require(annotated['sha'] == tag['object']['sha'] and annotated['tag'] == 'preview-' + SOURCE and annotated['object']['type'] == 'commit' and annotated['object']['sha'] == SOURCE, 'annotated_tag_source')
    workflow = value['workflow']
    source = value['source']
    verify_run(api(f'repos/{REPO}/actions/runs/{source["ci_run_id"]}'), source['ci_run_id'], source['ci_run_attempt'], SOURCE, '.github/workflows/ci.yml', 'push')
    for _ in range(120):
        signer_run = api(f'repos/{REPO}/actions/runs/{workflow["run_id"]}')
        if signer_run['status'] == 'completed':
            break
        require(signer_run['run_attempt'] == workflow['run_attempt'] and signer_run['head_sha'] == workflow['authority_sha'], 'pending_signer_identity')
        time.sleep(5)
    verify_run(signer_run, workflow['run_id'], workflow['run_attempt'], workflow['authority_sha'], WORKFLOW, 'workflow_run')
    ci = api(f'repos/{REPO}/actions/runs/{source["ci_run_id"]}')
    latest = latest_source_ci(ci['workflow_id'])
    require(latest['id'] == source['ci_run_id'] and latest['run_attempt'] == source['ci_run_attempt'], 'latest_ci')
    jobs = [job for page in pages(f'repos/{REPO}/actions/runs/{workflow["run_id"]}/attempts/{workflow["run_attempt"]}/jobs?per_page=100') for job in page['jobs']]
    signer = [job for job in jobs if job['name'] == 'preview-signer']
    require(len(signer) == 1 and signer[0]['run_id'] == workflow['run_id'] and signer[0]['run_attempt'] == workflow['run_attempt'] and signer[0]['head_sha'] == workflow['authority_sha'] and signer[0]['status'] == 'completed' and signer[0]['conclusion'] == 'success', 'signer_job')
    branch = api(f'repos/{REPO}/branches/main')
    require(branch['protected'] is True, 'main_protection')
    ancestry(SOURCE, workflow['authority_sha'])
    ancestry(workflow['authority_sha'], branch['commit']['sha'])
    with tempfile.TemporaryDirectory(prefix='velnor-sdk-preview-',dir=os.environ['RUNNER_TEMP']) as temporary:
        directory = Path(temporary)
        content = {name:download(row) for name,row in listing.items()}
        require(content['release.json'] == raw, 'receipt_changed')
        envelope = f'velnor-package-release-recovery-v1\nreceipt_sha256={digest(raw)}\nreceipt={base64.urlsafe_b64encode(raw).decode().rstrip("=")}\nbundle={base64.urlsafe_b64encode(content["release.json.bundle"]).decode().rstrip("=")}\n'
        require(len(envelope.encode()) <= 110 * 1024 and release['body'] == envelope and annotated['message'] == envelope, 'signed_recovery_envelope')
        for asset in value['assets']:
            require(len(content[asset['name']]) == asset['size'] and digest(content[asset['name']]) == asset['sha256'], 'signed_asset_digest')
        for name,data in content.items():
            (directory/name).write_bytes(data)
        cosign = os.environ['VELNOR_BOOTSTRAP_COSIGN']
        uri = f'https://github.com/{REPO}/{WORKFLOW}@refs/heads/main'
        subjects = {name:digest(content[name]) for name in value['evidence']['attested_subject_names']}
        for name in subjects:
            command([cosign,'verify-blob','--bundle',str(directory/(name+'.bundle')),'--certificate-identity',uri,'--certificate-oidc-issuer',ISSUER,'--certificate-github-workflow-repository',REPO,'--certificate-github-workflow-ref','refs/heads/main','--certificate-github-workflow-sha',workflow['authority_sha'],'--certificate-github-workflow-trigger','workflow_run',str(directory/name)])
            verify_attestation(name,content[name],subjects,value,directory)
        for product in value['products']:
            name = product['payloads'][0]['name']
            require(content[name+'.sha256'] == f'{digest(content[name])}  {name}\n'.encode(), 'checksum_sidecar')
            sbom = parse(content[name+'.sbom.json'])
            component = sbom['metadata']['component']
            require(sbom['bomFormat'] == 'CycloneDX' and component['type'] == 'file' and component['name'] == name and component['hashes'] == [{'alg':'SHA-256','content':digest(content[name])}], 'sbom_binding')
        require(content['SHA256SUMS'] == ''.join(f'{subjects[n]}  {n}\n' for n in sorted(subjects) if n != 'release.json').encode(), 'checksums')
        cli = extract(content['velnor-actions-'+TARGET+'.tar.gz'], 'velnor-actions')
        extract(content['velnor-archive-guard-'+TARGET+'.tar.gz'], 'velnor-archive-guard')
        require(assets(release['id']) == listing, 'release_changed')
        verify_run(api(f'repos/{REPO}/actions/runs/{source["ci_run_id"]}'), source['ci_run_id'], source['ci_run_attempt'], SOURCE, '.github/workflows/ci.yml', 'push')
        latest = latest_source_ci(ci['workflow_id'])
        require(latest['id'] == source['ci_run_id'] and latest['run_attempt'] == source['ci_run_attempt'], 'latest_ci_changed')
        current_branch = api(f'repos/{REPO}/branches/main')
        require(current_branch['protected'] is True, 'main_protection_changed')
        ancestry(workflow['authority_sha'],current_branch['commit']['sha'])
        output = Path(os.environ['RUNNER_TEMP'])/'velnor'/'bin'/('velnor-actions-' + BASE_VERSION)
        for parent in [output.parent.parent, output.parent]:
            require(not parent.is_symlink(), 'destination_parent_link')
            parent.mkdir(mode=0o700,exist_ok=True)
            require(parent.is_dir() and parent.stat().st_uid == os.geteuid(), 'destination_parent_owner')
        require(not output.exists() and not output.is_symlink(), 'destination_exists')
        with output.open('xb') as executable:
            executable.write(cli)
        output.chmod(0o755)


if __name__ == '__main__':
    main()
