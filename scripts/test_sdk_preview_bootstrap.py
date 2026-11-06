# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

ROOT = Path(os.environ['VELNOR_SDK_BOOTSTRAP_TEST_ROOT']) if 'VELNOR_SDK_BOOTSTRAP_TEST_ROOT' in os.environ else Path(__file__).resolve().parents[1]
SOURCE = Path(os.environ['VELNOR_SDK_BOOTSTRAP_VERIFIER']) if 'VELNOR_SDK_BOOTSTRAP_VERIFIER' in os.environ else ROOT / 'crates/velnor-actions-workflow-renderer/src/schema2_sdk_preview_bootstrap.py'
spec = importlib.util.spec_from_file_location('sdk_preview_bootstrap', SOURCE)
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)
A, B, H = 'a' * 40, 'b' * 40, 'c' * 40
REPO = bootstrap.REPO
WORKFLOW = bootstrap.WORKFLOW


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def archive(name):
    binary = b'\x7fELF\x02\x01' + b'\x00' * 12 + b'\x3e\x00' + b'\x00' * 44
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w:gz', format=tarfile.GNU_FORMAT) as tar:
        member = tarfile.TarInfo(name)
        member.size, member.mode = len(binary), 0o755
        tar.addfile(member, io.BytesIO(binary))
    return output.getvalue(), binary


def fixture():
    products, assets, content = [], [], {}
    for name, package in [('velnor-actions','velnor-actions-cli'),('velnor-archive-guard','velnor-archive-guard')]:
        payload = name + '-' + bootstrap.TARGET + '.tar.gz'
        raw, _ = archive(name)
        content[payload] = raw
        content[payload + '.bundle'] = b'{}'
        content[payload + '.sha256'] = f'{sha(raw)}  {payload}\n'.encode()
        content[payload + '.sbom.json'] = canonical({'bomFormat':'CycloneDX','metadata':{'component':{'type':'file','name':payload,'hashes':[{'alg':'SHA-256','content':sha(raw)}]}}})
        products.append({'id':name,'package':package,'package_version':'0.1.75','bins':[name],'payloads':[{'format':'tar-gz','target':bootstrap.TARGET,'name':payload,'members':[name]}]})
        for suffix, kind in [('', 'payload'),('.bundle','signature_bundle'),('.sha256','sha256'),('.sbom.json','sbom')]:
            key = payload + suffix
            assets.append({'name':key,'kind':kind,'product_id':name,'target':bootstrap.TARGET,'subject_name':payload,'size':len(content[key]),'sha256':sha(content[key])})
    payloads = sorted(p['payloads'][0]['name'] for p in products)
    content['SHA256SUMS'] = ''.join(f'{sha(content[name])}  {name}\n' for name in payloads).encode()
    assets.append({'name':'SHA256SUMS','kind':'checksum_list','product_id':None,'target':None,'subject_name':None,'size':len(content['SHA256SUMS']),'sha256':sha(content['SHA256SUMS'])})
    receipt = {'schema':'velnor.package-release.v2','channel':'preview','repository':{'slug':REPO,'id':1},'source':{'commit_sha':A,'ref':'refs/heads/main','default_branch':'main','event':'workflow_run','ci_run_id':2,'ci_run_attempt':1},'workflow':{'path':WORKFLOW,'ref':'refs/heads/main','authority_sha':B,'run_id':3,'run_attempt':1,'producer_run_id':3,'producer_run_attempt':1,'signer_job_id':'preview-signer','signer_workflow':WORKFLOW},'version':{'base':'0.1.75','value':'0.1.75-preview.4+aaaaaaa','commit_count':4,'source_sha7':'aaaaaaa'},'tag':'preview-'+A,'products':products,'companions':[],'assets':sorted(assets,key=lambda a:a['name']),'evidence':{'profile':'signed-sbom-v1','sbom_format':'CycloneDX-JSON','signer_issuer':bootstrap.ISSUER,'signer_workflow_ref':f'{REPO}/{WORKFLOW}@refs/heads/main','attestation_predicate':bootstrap.PREDICATE,'attested_subject_names':sorted(payloads+['release.json'])}}
    content['release.json'] = canonical(receipt)
    content['release.json.bundle'] = b'{}'
    repository = {'full_name':REPO,'id':1,'default_branch':'main'}
    release = {'id':7,'tag_name':'preview-'+A,'draft':False,'prerelease':True,'immutable':True,'author':{'id':41898282,'login':'github-actions[bot]'},'body':f'velnor-package-release-recovery-v1\nreceipt_sha256={sha(content["release.json"])}\nreceipt={base64.urlsafe_b64encode(content["release.json"]).decode().rstrip("=")}\nbundle={base64.urlsafe_b64encode(content["release.json.bundle"]).decode().rstrip("=")}\n'}
    rows = [{'id':i+1,'name':name,'size':len(raw),'state':'uploaded','digest':'sha256:'+sha(raw)} for i,(name,raw) in enumerate(sorted(content.items()))]
    ci = {'id':2,'run_attempt':1,'head_sha':A,'path':'.github/workflows/ci.yml','workflow_id':8,'event':'push','status':'completed','conclusion':'success','head_branch':'main','repository':repository,'head_repository':repository,'run_number':11}
    signer = dict(ci,id=3,head_sha=B,path=WORKFLOW,workflow_id=9,event='workflow_run')
    responses = {}
    def response(args,value):
        responses[json.dumps(args)] = base64.b64encode(canonical(value)).decode()
    def api(endpoint,value):
        response(['api',endpoint],value)
    def pages(endpoint,value):
        response(['api','--paginate','--slurp',endpoint],value)
    api('repos/'+REPO,repository)
    pages(f'repos/{REPO}/releases?per_page=100',[[release]])
    pages(f'repos/{REPO}/releases/7/assets?per_page=100',[rows[:5],rows[5:]])
    for row in rows:
        responses[json.dumps(['api','-H','Accept: application/octet-stream',f'repos/{REPO}/releases/assets/{row["id"]}'])] = base64.b64encode(content[row['name']]).decode()
    api(f'repos/{REPO}/git/ref/tags/preview-{A}',{'ref':'refs/tags/preview-'+A,'object':{'sha':'d'*40,'type':'tag'}})
    api(f'repos/{REPO}/git/tags/'+ 'd'*40,{'sha':'d'*40,'tag':'preview-'+A,'object':{'sha':A,'type':'commit'},'message':release['body']})
    api(f'repos/{REPO}/actions/runs/2',ci)
    api(f'repos/{REPO}/actions/runs/3',signer)
    for run in [ci,signer]:
        api(f'repos/{REPO}/actions/workflows/{run["workflow_id"]}',{'id':run['workflow_id'],'path':run['path'],'state':'active'})
    pages(f'repos/{REPO}/actions/workflows/8/runs?head_sha={A}&per_page=100',[{'workflow_runs':[ci]}])
    pages(f'repos/{REPO}/actions/runs/3/attempts/1/jobs?per_page=100',[{'jobs':[{'name':'preview-signer','run_id':3,'run_attempt':1,'head_sha':B,'status':'completed','conclusion':'success'}]}])
    api(f'repos/{REPO}/branches/main',{'protected':True,'commit':{'sha':H}})
    for base,head in [(A,B),(B,H)]:
        api(f'repos/{REPO}/compare/{base}...{head}',{'status':'ahead','base_commit':{'sha':base},'merge_base_commit':{'sha':base}})
    uri = f'https://github.com/{REPO}/{WORKFLOW}@refs/heads/main'
    certificate = {'issuer':bootstrap.ISSUER,'subjectAlternativeName':uri,'sourceRepositoryURI':f'https://github.com/{REPO}','sourceRepositoryOwnerURI':'https://github.com/tailrocks','sourceRepositoryIdentifier':'1','sourceRepositoryDigest':A,'sourceRepositoryRef':'refs/heads/main','buildSignerURI':uri,'buildSignerDigest':B,'buildConfigURI':uri,'buildConfigDigest':B,'buildTrigger':'workflow_run','runnerEnvironment':'github-hosted','runInvocationURI':f'https://github.com/{REPO}/actions/runs/3/attempts/1'}
    verified = [{'verificationResult':{'signature':{'certificate':certificate},'statement':{'predicateType':bootstrap.PREDICATE,'predicate':{'buildDefinition':{'internalParameters':{'github':{'event_name':'workflow_run'}}}},'subject':[{'name':name,'digest':{'sha256':sha(content[name])}} for name in sorted(payloads+['release.json'])]}}}]
    for name in payloads+['release.json']:
        pages(f'repos/{REPO}/attestations/sha256:{sha(content[name])}?per_page=100&predicate_type=https%3A%2F%2Fslsa.dev%2Fprovenance%2Fv1',[{'attestations':[{'bundle':{'signed':'fixture'}}]}])
    responses['attestation'] = base64.b64encode(canonical(verified)).decode()
    return responses, receipt, rows, content


class BootstrapTests(unittest.TestCase):
    def run_fixture(self, mutate=None, accepted=False):
        responses,receipt,rows,content = fixture()
        if mutate:
            mutate(responses,receipt,rows,content)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root/'responses.json').write_text(json.dumps(responses))
            gh = root/'gh'
            gh.write_text('#!'+sys.executable+'\nimport base64,json,os,sys\nfrom pathlib import Path\nr=json.loads(Path(os.environ["FIXTURE_RESPONSES"]).read_text())\na=sys.argv[1:]\nk="attestation" if a[0]=="attestation" else json.dumps(a)\nsys.stdout.buffer.write(base64.b64decode(r[k]))\n')
            gh.chmod(0o755)
            env = {'RUNNER_TEMP':str(root),'VELNOR_BOOTSTRAP_GH':str(gh),'VELNOR_BOOTSTRAP_COSIGN':'/usr/bin/true','VELNOR_IMAGE_SOURCE_SHA':A,'VELNOR_SDK_BASE_VERSION':'0.1.75','FIXTURE_RESPONSES':str(root/'responses.json')}
            destination = root/'velnor/bin/velnor-actions-0.1.75'
            with patch.dict(os.environ,env):
                if mutate and not accepted:
                    with self.assertRaises((ValueError,KeyError,tarfile.TarError)):
                        bootstrap.main()
                    self.assertFalse(destination.exists())
                else:
                    bootstrap.main()
                    self.assertEqual(destination.read_bytes(),archive('velnor-actions')[1])

    def test_verified_full_paginated_release_stages_archive_bytes_without_execution(self):
        self.run_fixture()

    def test_wrong_signed_digest_and_complete_inventory_are_rejected(self):
        def extra(responses,receipt,rows,content):
            rows.append(dict(rows[0],id=99,name='extra'))
            responses[json.dumps(['api','--paginate','--slurp',f'repos/{REPO}/releases/7/assets?per_page=100'])] = base64.b64encode(canonical([rows])).decode()
        self.run_fixture(extra)
        def changed(responses,receipt,rows,content):
            row = next(r for r in rows if r['name'].startswith('velnor-actions-') and r['name'].endswith('.tar.gz'))
            responses[json.dumps(['api','-H','Accept: application/octet-stream',f'repos/{REPO}/releases/assets/{row["id"]}'])] = base64.b64encode(b'x'*row['size']).decode()
        self.run_fixture(changed)

    def test_valid_crypto_for_wrong_signer_run_is_rejected(self):
        def wrong(responses,receipt,rows,content):
            verified = json.loads(base64.b64decode(responses['attestation']))
            verified[0]['verificationResult']['signature']['certificate']['runInvocationURI'] = f'https://github.com/{REPO}/actions/runs/999/attempts/1'
            responses['attestation'] = base64.b64encode(canonical(verified)).decode()
        self.run_fixture(wrong)

    def test_receipt_source_authority_and_closed_schema_rejections(self):
        def replace_receipt(responses, rows, raw):
            row = next(row for row in rows if row['name'] == 'release.json')
            row['size'], row['digest'] = len(raw), 'sha256:' + sha(raw)
            responses[json.dumps(['api','-H','Accept: application/octet-stream',f'repos/{REPO}/releases/assets/{row["id"]}'])] = base64.b64encode(raw).decode()
            responses[json.dumps(['api','--paginate','--slurp',f'repos/{REPO}/releases/7/assets?per_page=100'])] = base64.b64encode(canonical([rows[:5],rows[5:]])).decode()
        cases = [
            lambda receipt: receipt['source'].update(commit_sha=H),
            lambda receipt: receipt['workflow'].update(path='.github/workflows/publish.yml'),
            lambda receipt: receipt['workflow'].update(signer_job_id='other-signer'),
            lambda receipt: receipt['workflow'].update(producer_run_attempt=2),
            lambda receipt: receipt.update(unexpected=True),
        ]
        for change in cases:
            with self.subTest(change=change):
                def mutate(responses,receipt,rows,content):
                    change(receipt)
                    replace_receipt(responses,rows,canonical(receipt))
                self.run_fixture(mutate)
        for raw_transform in [lambda raw:b'{"channel":"preview",'+raw[1:], lambda raw:raw+b'\n']:
            def mutate(responses,receipt,rows,content):
                replace_receipt(responses,rows,raw_transform(content['release.json']))
            self.run_fixture(mutate)

    def test_stale_ci_attempt_and_invalid_ancestry_rejections(self):
        for endpoint,key,value in [
            (f'repos/{REPO}/actions/runs/2','run_attempt',2),
            (f'repos/{REPO}/branches/main','protected',False),
            (f'repos/{REPO}/compare/{A}...{B}','status','diverged'),
            (f'repos/{REPO}/git/ref/tags/preview-{A}','object',{'sha':A,'type':'commit'}),
            (f'repos/{REPO}/git/tags/'+ 'd'*40,'message','wrong-envelope'),
        ]:
            with self.subTest(endpoint=endpoint):
                def mutate(responses,receipt,rows,content):
                    route=json.dumps(['api',endpoint])
                    response=json.loads(base64.b64decode(responses[route]))
                    response[key]=value
                    responses[route]=base64.b64encode(canonical(response)).decode()
                self.run_fixture(mutate)

    def test_bounded_snappy_literal_copy_overlap_and_invalid_encodings(self):
        source=b'abcdefgh'
        for encoded in [b'\x10\x1c'+source+b'\x11\x08',b'\x10\x1c'+source+b'\x1e\x08\x00',b'\x10\x1c'+source+b'\x1f\x08\x00\x00\x00']:
            self.assertEqual(bootstrap.decode_snappy(encoded),source*2)
        self.assertEqual(bootstrap.decode_snappy(b'\x06\x00a\x12\x01\x00'),b'a'*6)
        self.assertEqual(bootstrap.decode_snappy(b'\x40\xf0\x3f'+b'a'*64),b'a'*64)
        invalid=[b'',b'\x00',b'\x80',b'\x80\x80\x80\x80\x10',b'\x81\x80\x80\x04',b'\x02\x00a',b'\x05\xf0',b'\x02\x00a\x03',b'\x02\x00a\x02\x00\x00',b'\x02\x00a\x02\x02\x00',b'\x01\x00a\x00',b'x'*(bootstrap.MAX_API+1)]
        for encoded in invalid:
            with self.subTest(encoded=encoded[:20]):
                with self.assertRaises(ValueError):
                    bootstrap.decode_snappy(encoded)

    def test_external_snappy_bundle_transport_is_unauthenticated_and_verified(self):
        raw=base64.b64decode('FEx7InNpZ25lZCI6ImZpeHR1cmUifQ==')
        self.assertEqual(bootstrap.decode_snappy(raw),b'{"signed":"fixture"}')
        class ExternalResponse(io.BytesIO):
            status=200
            headers={'Content-Encoding':'identity'}
        requests=[]
        def read_external(opener,request,timeout):
            self.assertEqual(request.full_url,'https://fixture.blob.core.windows.net/bundle')
            self.assertIsNone(request.get_header('Authorization'))
            self.assertEqual(request.get_header('Accept-encoding'),'identity')
            requests.append(request)
            return ExternalResponse(raw)
        def external_urls(responses,receipt,rows,content):
            for key in list(responses):
                if '/attestations/sha256:' in key:
                    responses[key]=base64.b64encode(canonical([{'attestations':[{'bundle_url':'https://fixture.blob.core.windows.net/bundle'}]}])).decode()
        with patch('urllib.request.OpenerDirector.open',new=read_external):
            self.run_fixture(external_urls,accepted=True)
        self.assertEqual(len(requests),3)

    def test_tar_extra_member_is_rejected_before_extract(self):
        raw,binary = archive('velnor-actions')
        output = io.BytesIO()
        with tarfile.open(fileobj=output,mode='w:gz',format=tarfile.GNU_FORMAT) as tar:
            for name in ['velnor-actions','velnor-archive-guard']:
                member=tarfile.TarInfo(name)
                member.size,member.mode=len(binary),0o755
                tar.addfile(member,io.BytesIO(binary))
        with self.assertRaises(ValueError):
            bootstrap.extract(output.getvalue(),'velnor-actions')
        for name,kind,size in [('other',tarfile.REGTYPE,len(binary)),('../velnor-actions',tarfile.REGTYPE,len(binary)),('velnor-actions',tarfile.SYMTYPE,0),('velnor-actions',tarfile.REGTYPE,bootstrap.MAX_ARCHIVE+1)]:
            with self.subTest(name=name,kind=kind,size=size):
                member=tarfile.TarInfo(name)
                member.type,member.size,member.mode=kind,size,0o755
                raw=member.tobuf(format=tarfile.GNU_FORMAT)
                raw+=binary if size == len(binary) else b''
                import gzip
                with self.assertRaises((ValueError,tarfile.TarError)):
                    bootstrap.extract(gzip.compress(raw),'velnor-actions')



if __name__ == '__main__':
    unittest.main()
