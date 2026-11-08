# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import contextlib
import copy
import io
import json
import os
import pathlib
import tempfile
import unittest
import urllib.parse
from unittest.mock import patch

ROOT = pathlib.Path(__file__).resolve().parents[1] / 'src'
SCOPE = {}
generated = os.environ.get('VELNOR_PREMERGE_TEST_SOURCE')
code = pathlib.Path(generated).read_text() if generated else (ROOT / 'premerge_event_probe.py').read_text()
if generated:
    suffix = 'sys.exit(main())\n'
    if not code.endswith(suffix):
        raise RuntimeError('invalid generated source')
    code = code[:-len(suffix)]
exec(compile(code, 'premerge_event_probe.py', 'exec'), SCOPE)

HEAD, BASE, MERGE, CALLER, SIGNER, TREE = (character * 40 for character in 'abcdef')
REPOSITORY = {'id': 7, 'full_name': 'example/project', 'owner': {'id': 11, 'login': 'example'}, 'default_branch': 'main'}


def context():
    return {'VELNOR_PROOF_REPOSITORY': 'example/project', 'VELNOR_PROOF_REPOSITORY_ID': '7',
            'VELNOR_PROOF_OWNER_ID': '11', 'VELNOR_PROOF_BASE_BRANCH': 'main',
            'VELNOR_PROOF_CALLER_WORKFLOW': '.github/workflows/ci.yml',
            'VELNOR_PROOF_ENVIRONMENT': 'premerge-proof', 'VELNOR_PROOF_REVIEWER_ID': '60',
            'GITHUB_REPOSITORY': 'example/project', 'GITHUB_EVENT_NAME': 'pull_request',
            'GITHUB_REF': 'refs/pull/42/merge', 'GITHUB_SHA': MERGE, 'GITHUB_WORKFLOW_SHA': CALLER,
            'GITHUB_WORKFLOW_REF': 'example/project/.github/workflows/ci.yml@refs/pull/42/merge',
            'VELNOR_PROOF_SIGNER_SHA': SIGNER, 'VELNOR_PROOF_SIGNER_REPOSITORY': 'example/project',
            'VELNOR_PROOF_SIGNER_PATH': '.github/workflows/premerge-event-proof.yml',
            'VELNOR_PROOF_SIGNER_REF': 'example/project/.github/workflows/premerge-event-proof.yml@' + SIGNER,
            'GITHUB_RUN_ID': '30', 'GITHUB_RUN_ATTEMPT': '1', 'VELNOR_PROOF_JOB_ID': '31'}


def event():
    return {'repository': copy.deepcopy(REPOSITORY), 'number': 42,
            'pull_request': {'number': 42, 'state': 'open', 'draft': False, 'merged': False,
                'merge_commit_sha': MERGE, 'head': {'sha': HEAD, 'ref': 'feature', 'repo': copy.deepcopy(REPOSITORY)},
                'base': {'sha': BASE, 'ref': 'main', 'repo': copy.deepcopy(REPOSITORY)}}}


class Api:
    def __init__(self):
        prefix = 'repos/example/project'
        self.values = {prefix: copy.deepcopy(REPOSITORY), prefix + '/pulls/42': event()['pull_request'],
            prefix + '/git/commits/' + MERGE: {'sha': MERGE, 'tree': {'sha': TREE}, 'parents': [{'sha': BASE}, {'sha': HEAD}]},
            prefix + '/git/commits/' + CALLER: {'sha': CALLER},
            prefix + '/actions/runs/30': {'id': 30, 'run_attempt': 1, 'event': 'pull_request', 'head_sha': HEAD,
                'head_branch': 'feature', 'path': '.github/workflows/ci.yml',
                'repository': copy.deepcopy(REPOSITORY), 'head_repository': copy.deepcopy(REPOSITORY)},
            prefix + '/actions/jobs/31': {'id': 31, 'run_id': 30, 'run_attempt': 1, 'status': 'in_progress'},
            prefix + '/environments/premerge-proof': {'id': 50, 'name': 'premerge-proof', 'can_admins_bypass': False,
                'deployment_branch_policy': {'custom_branch_policies': True},
                'protection_rules': [{'type': 'required_reviewers', 'reviewers': [{'type': 'User', 'reviewer': {'id': 60, 'login': 'reviewer'}}]}]},
            prefix + '/environments/premerge-proof/deployment-branch-policies?per_page=100': {
                'total_count': 1, 'branch_policies': [{'type': 'branch', 'name': 'refs/pull/*/merge'}]},
            prefix + '/actions/runs/30/approvals': [{'state': 'approved', 'user': {'id': 60, 'type': 'User'}, 'environments': [{'id': 50}]}]}
        self.calls = []

    def get(self, path):
        self.calls.append(path)
        return copy.deepcopy(self.values[path])


class Response(io.BytesIO):
    status = 200
    def __enter__(self):
        return self
    def __exit__(self, *_args):
        self.close()


class Remote(Api):
    def open(self, request, timeout):
        self.calls.append((request.full_url, timeout))
        url = urllib.parse.urlsplit(request.full_url)
        if url.scheme != 'https' or url.hostname != 'api.github.com' or request.method != 'GET':
            raise RuntimeError('unexpected endpoint')
        route = url.path.lstrip('/') + ('?' + url.query if url.query else '')
        return Response(json.dumps(self.values[route]).encode())


class PremergeProof(unittest.TestCase):
    def receipt(self, api):
        expected = SCOPE['context'](context())
        observed = SCOPE['parse_event'](json.dumps(event()).encode(), expected)
        return SCOPE['receipt'](api, expected, observed)

    def test_own_event_records_distinct_source_caller_and_signer(self):
        result = self.receipt(Api())
        self.assertEqual(result['head_sha'], HEAD)
        self.assertEqual(result['base_sha'], BASE)
        self.assertEqual(result['merge_sha'], MERGE)
        self.assertEqual(result['merge_tree_sha'], TREE)
        self.assertEqual(result['caller_sha'], CALLER)
        self.assertEqual(result['signer_sha'], SIGNER)
        self.assertEqual(result['job_id'], 31)
        self.assertNotIn('checks', result)

    def test_context_rejects_wrong_event_fork_ref_attempt_and_mutable_callee(self):
        for key, bad in [('GITHUB_EVENT_NAME', 'pull_request_target'), ('GITHUB_REPOSITORY', 'fork/project'),
                         ('GITHUB_REF', 'refs/heads/main'), ('GITHUB_RUN_ATTEMPT', '2'),
                         ('GITHUB_WORKFLOW_REF', 'example/project/.github/workflows/other.yml@refs/pull/42/merge'),
                         ('VELNOR_PROOF_SIGNER_REF', 'example/project/.github/workflows/premerge-event-proof.yml@refs/heads/main'),
                         ('VELNOR_PROOF_SIGNER_REPOSITORY', 'fork/project'), ('VELNOR_PROOF_JOB_ID', '0')]:
            with self.subTest(key=key):
                env = context()
                env[key] = bad
                with self.assertRaises(ValueError):
                    SCOPE['context'](env)

    def test_current_event_merge_parents_and_job_substitutions_reject(self):
        prefix = 'repos/example/project'
        cases = [(prefix + '/pulls/42', 'draft', True), (prefix + '/pulls/42', 'state', 'closed'),
                 (prefix + '/actions/runs/30', 'event', 'push'),
                 (prefix + '/actions/runs/30', 'head_sha', MERGE),
                 (prefix + '/actions/jobs/31', 'run_attempt', 2),
                 (prefix + '/actions/jobs/31', 'status', 'completed'),
                 (prefix + '/git/commits/' + MERGE, 'parents', [{'sha': HEAD}, {'sha': BASE}]),
                 (prefix + '/git/commits/' + MERGE, 'parents', [{'sha': BASE}])]
        for path, key, bad in cases:
            with self.subTest(path=path, key=key):
                api = Api()
                api.values[path][key] = bad
                with self.assertRaises(ValueError):
                    self.receipt(api)

    def test_stale_head_base_and_fork_reject(self):
        for side, key, bad in [('head', 'sha', 'f' * 40), ('base', 'sha', 'f' * 40),
                               ('base', 'ref', 'other'), ('head', 'repo', {'id': 99})]:
            with self.subTest(side=side, key=key):
                api = Api()
                api.values['repos/example/project/pulls/42'][side][key] = bad
                with self.assertRaises(ValueError):
                    self.receipt(api)

    def test_bypass_wrong_reviewer_main_only_environment_and_missing_approval_reject(self):
        for mutation in ('bypass', 'reviewer', 'branch', 'approval'):
            api = Api()
            environment = api.values['repos/example/project/environments/premerge-proof']
            if mutation == 'bypass':
                environment['can_admins_bypass'] = True
            elif mutation == 'reviewer':
                environment['protection_rules'][0]['reviewers'][0]['reviewer']['id'] = 61
            elif mutation == 'branch':
                api.values['repos/example/project/environments/premerge-proof/deployment-branch-policies?per_page=100']['branch_policies'][0]['name'] = 'main'
            else:
                api.values['repos/example/project/actions/runs/30/approvals'] = []
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.receipt(api)

    def test_reobservation_rejects_head_drift(self):
        class Drift(Api):
            def get(self, route):
                result = super().get(route)
                if route.endswith('/approvals'):
                    self.values['repos/example/project/pulls/42']['head']['sha'] = 'f' * 40
                return result
        with self.assertRaises(ValueError):
            self.receipt(Drift())

    def test_generated_final_operation_writes_own_event_receipt(self):
        directory = os.environ.get('VELNOR_PREMERGE_TEST_DIRECTORY')
        if not directory:
            self.skipTest('requires rendered materialized source')
        loader = {}
        path = pathlib.Path(os.environ['VELNOR_PREMERGE_TEST_LOADER'])
        exec(compile(path.read_text(), str(path), 'exec'), loader)
        root = pathlib.Path(directory)
        event_file = root.parent / 'native-event.json'
        event_file.write_text(json.dumps(event()))
        env = context()
        env.update(RUNNER_TEMP=str(root.parent), VELNOR_PREMERGE_SOURCE=str(root),
                   GITHUB_EVENT_PATH=str(event_file), GIT_TOKEN='private-token-sentinel')
        count = int(os.environ['VELNOR_PREMERGE_TEST_COUNT'])
        digest = os.environ['VELNOR_PREMERGE_TEST_DIGEST']
        with patch.dict(os.environ, env, clear=True), patch('urllib.request.build_opener', return_value=Remote()), patch('ssl.create_default_context', return_value=object()):
            with self.assertRaises(SystemExit) as result:
                loader['execute_source'](count, digest)
        self.assertEqual(result.exception.code, 0)
        data = (root / 'receipt.json').read_bytes()
        self.assertNotIn(b'private-token-sentinel', data)
        self.assertEqual(json.loads(data)['merge_sha'], MERGE)

    def test_full_entrypoint_writes_only_bounded_context_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary).resolve()
            path = root / 'event.json'
            path.write_text(json.dumps(event()))
            env = context()
            env.update(GITHUB_EVENT_PATH=str(path), VELNOR_PREMERGE_SOURCE=str(root), GIT_TOKEN='private-token-sentinel')
            with patch.dict(os.environ, env, clear=True), patch('urllib.request.build_opener', return_value=Remote()), patch('ssl.create_default_context', return_value=object()):
                self.assertEqual(SCOPE['main'](), 0)
            data = (root / 'receipt.json').read_bytes()
            self.assertLess(len(data), 16384)
            self.assertNotIn(b'private-token-sentinel', data)
            self.assertEqual(json.loads(data)['scenario'], 'premerge-event')

    def test_failure_is_secret_free_and_writes_no_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary).resolve()
            path = root / 'event.json'
            path.write_text('{"number":1,"number":2}')
            env = context()
            env.update(GITHUB_EVENT_PATH=str(path), VELNOR_PREMERGE_SOURCE=str(root), GIT_TOKEN='private-token-sentinel')
            error = io.StringIO()
            with patch.dict(os.environ, env, clear=True), contextlib.redirect_stderr(error):
                self.assertEqual(SCOPE['main'](), 1)
            self.assertEqual(error.getvalue(), 'premerge event proof failed\n')
            self.assertFalse((root / 'receipt.json').exists())
        with self.assertRaises(ValueError):
            SCOPE['decode'](b' ' * (SCOPE['MAX_JSON'] + 1))


if __name__ == '__main__':
    unittest.main()
