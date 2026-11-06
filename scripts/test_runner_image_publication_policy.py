# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(os.environ['VELNOR_IMAGE_POLICY_TEST_ROOT']) if 'VELNOR_IMAGE_POLICY_TEST_ROOT' in os.environ else Path(__file__).resolve().parents[1]
POLICY = Path(os.environ.get('VELNOR_IMAGE_POLICY_TEST_SCRIPT', ROOT / 'crates/velnor-actions-workflow-renderer/src/schema2_runner_image_publication_policy.sh'))
SOURCE = 'a' * 40
DIGEST = 'sha256:' + 'b' * 64
CONFIG = 'sha256:' + 'c' * 64
REPOSITORY = 'ghcr.io/tailrocks/velnor-runner'


def fixtures():
    journal = [{'status':'pending'}, {'schema':'velnor.image-publication.v1','status':'verified',
        'source_commit':SOURCE,'source_ref':'refs/heads/main','profile':'runner','repository':REPOSITORY,
        'tag':'sha-'+SOURCE,'workflow_path':'.github/workflows/publish.yml','workflow_authority_sha':SOURCE,
        'producer_run_id':2,'producer_run_attempt':1,'ci_run_id':2,'ci_run_attempt':1,
        'registry_digest':DIGEST,'platform_manifest_digest':DIGEST,'image_configuration_digest':CONFIG}]
    candidate = {'oci':{'platform_manifest_digest':DIGEST,'image_configuration_digest':CONFIG}}
    jobs = [{'jobs':[{'name':'Publish native image (runner)','run_id':3,'run_attempt':1,'head_sha':SOURCE,'status':'completed','conclusion':'success'}]}]
    attestations = [{'verificationResult':{'signature':{'certificate':{
        'runInvocationURI':'https://github.com/tailrocks/velnor-new/actions/runs/3/attempts/1',
        'sourceRepositoryIdentifier':'1390620900','sourceRepositoryDigest':SOURCE,'buildSignerDigest':SOURCE,'runnerEnvironment':'github-hosted','buildTrigger':'push'}},
        'statement':{'predicateType':'https://slsa.dev/provenance/v1','subject':[{'name':REPOSITORY,'digest':{'sha256':DIGEST[7:]}}]}}}]
    return journal,candidate,jobs,attestations


class PublicationPolicyTests(unittest.TestCase):
    def run_policy(self, documents):
        journal,candidate,jobs,attestations=documents
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            (root/'journal').write_text(''.join(json.dumps(row)+'\n' for row in journal))
            for name,value in [('candidate',candidate),('jobs',jobs),('attestations',attestations)]:
                (root/name).write_text(json.dumps(value))
            script='set -euo pipefail\nsource "$1"\nvalidate_journal "$2/journal" "$2/candidate"\nvalidate_publisher_jobs "$2/jobs"\nvalidate_digest_attestation "$2/attestations"'
            env=os.environ|{'sha':SOURCE,'profile':'runner','role':'runner','repository':REPOSITORY,'repo':'tailrocks/velnor-new',
                'ci_id':'2','ci_attempt':'1','publish_id':'3','publish_attempt':'1','digest':DIGEST}
            result=subprocess.run(['bash','-c',script,'policy',str(POLICY),str(root)],env=env,capture_output=True)
            return result

    def test_exact_published_digest_is_joined_to_source_ci_and_publisher(self):
        result=self.run_policy(fixtures())
        self.assertEqual(result.returncode,0,result.stderr.decode())
        self.assertEqual(result.stdout.decode(),DIGEST+'\n')

    def test_journal_substitutions_are_rejected(self):
        for field,value in [('source_commit','d'*40),('workflow_authority_sha','d'*40),('producer_run_id',4),
            ('producer_run_attempt',2),('ci_run_id',4),('ci_run_attempt',2),('profile','dind'),('repository','ghcr.io/other/image'),
            ('registry_digest','sha256:'+'d'*64),('platform_manifest_digest','sha256:'+'d'*64),('image_configuration_digest','sha256:'+'d'*64)]:
            with self.subTest(field=field):
                documents=fixtures();documents[0][1][field]=value
                self.assertNotEqual(self.run_policy(documents).returncode,0)

    def test_publisher_identity_and_success_are_required(self):
        for field,value in [('name','other'),('run_id',4),('run_attempt',2),('head_sha','d'*40),('status','in_progress'),('conclusion','failure')]:
            with self.subTest(field=field):
                documents=fixtures();documents[2][0]['jobs'][0][field]=value
                self.assertNotEqual(self.run_policy(documents).returncode,0)
        documents=fixtures();documents[2][0]['jobs']*=2
        self.assertNotEqual(self.run_policy(documents).returncode,0)

    def test_verified_attestation_must_identify_the_same_publisher_and_digest(self):
        for field,value in [('runInvocationURI','https://github.com/tailrocks/velnor-new/actions/runs/4/attempts/1'),
            ('sourceRepositoryIdentifier','999'),('sourceRepositoryDigest','d'*40),('buildSignerDigest','d'*40),('runnerEnvironment','self-hosted'),('buildTrigger','workflow_dispatch')]:
            with self.subTest(field=field):
                documents=fixtures();documents[3][0]['verificationResult']['signature']['certificate'][field]=value
                self.assertNotEqual(self.run_policy(documents).returncode,0)
        for field,value in [('name','other'),('digest',{'sha256':'d'*64})]:
            documents=fixtures();documents[3][0]['verificationResult']['statement']['subject'][0][field]=value
            self.assertNotEqual(self.run_policy(documents).returncode,0)
        documents=fixtures();documents[3].extend(documents[3][:])
        self.assertNotEqual(self.run_policy(documents).returncode,0)

    def test_publisher_job_pagination_is_complete_consistent_and_unique(self):
        first=[{'id':index+1} for index in range(100)]
        second=[{'id':index+101} for index in range(3)]
        valid=[{'total_count':103,'jobs':first},{'total_count':103,'jobs':second}]
        cases={'valid':valid,
            'truncated':[{'total_count':104,'jobs':first},{'total_count':104,'jobs':second}],
            'changed-total':[valid[0],{'total_count':104,'jobs':second}],
            'duplicate':[valid[0],{'total_count':103,'jobs':[{'id':1}]+second[1:]}],
            'overflow':[{'total_count':2001,'jobs':first}]}
        for case,pages in cases.items():
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary)
                (root/'pages.json').write_text(json.dumps(pages))
                transport=root/'transport.py'
                transport.write_text('import json,sys,urllib.parse\nfrom pathlib import Path\npages=json.loads(Path(sys.argv[1]).read_text())\npage=int(urllib.parse.parse_qs(urllib.parse.urlsplit(sys.argv[-1]).query)["page"][0])\nprint(json.dumps(pages[page-1]))\n')
                script='set -euo pipefail\nsource "$1"\ngh() { python3 "$base/transport.py" "$base/pages.json" "$@"; }\nfetch_publisher_jobs'
                env=os.environ|{'base':str(root),'repo':'tailrocks/velnor-new','publish_id':'3','publish_attempt':'1'}
                result=subprocess.run(['bash','-c',script,'policy',str(POLICY)],env=env,capture_output=True)
                self.assertEqual(result.returncode == 0,case == 'valid',result.stderr.decode())
                if case == 'valid':
                    self.assertEqual(len(json.loads((root/'jobs.json').read_text())[0]['jobs']),103)


if __name__ == '__main__':
    unittest.main()
