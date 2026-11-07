# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import re
from urllib.parse import quote


class AdmissionError(Exception):
    pass


def require(condition):
    if not condition:
        raise AdmissionError('OIDC qualification admission failed')


def positive(value):
    return type(value) is int and value > 0


def sha(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{40}', value) is not None


def field(value, *path):
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def pages(api, route, member=None):
    seen = 0
    expected = None
    for page in range(1, 101):
        value = api.get(f'{route}{"&" if "?" in route else "?"}per_page=100&page={page}')
        if member is None:
            items = value
        else:
            total = field(value, 'total_count')
            require(type(total) is int and 0 <= total <= 10000)
            require(expected is None or expected == total)
            expected = total
            items = field(value, member)
        require(isinstance(items, list) and len(items) <= 100)
        seen += len(items)
        require(seen <= 10000 and (expected is None or seen <= expected))
        yield items
        if expected is None and len(items) < 100:
            return
        if expected is not None and seen == expected:
            return
        require(len(items) > 0)
    raise AdmissionError('OIDC qualification pagination limit')



def valid_repository(value):
    if not isinstance(value, str) or value.count('/') != 1:
        return False
    owner, name = value.split('/')
    return (0 < len(owner) <= 39 and not owner.startswith('-') and not owner.endswith('-')
            and '--' not in owner and re.fullmatch(r'[A-Za-z0-9-]+', owner) is not None
            and 0 < len(name) <= 100 and name not in ('.', '..') and not name.endswith('.git')
            and re.fullmatch(r'[A-Za-z0-9_.-]+', name) is not None)


def valid_branch(value):
    if (not isinstance(value, str) or not value or len(value) > 255 or value == 'HEAD'
            or '..' in value or re.fullmatch(r'[A-Za-z0-9/_.-]+', value) is None):
        return False
    return all(part and not part.startswith(('.', '-')) and not part.endswith('.')
               and not part.endswith('.lock') for part in value.split('/'))

def reviewed_merge(api, request):
    repository = request['repository']
    branch = request['default_branch']
    workflow_sha = request['workflow_sha']
    require(valid_repository(repository) and valid_branch(branch))
    require(sha(workflow_sha))
    prefix = f'repos/{repository}'
    repo = api.get(prefix)
    repo_id = field(repo, 'id')
    require(positive(repo_id) and field(repo, 'full_name') == repository and field(repo, 'default_branch') == branch)
    selected = None
    for items in pages(api, f'{prefix}/commits/{workflow_sha}/pulls'):
        for pull in items:
            if (field(pull, 'merged_at') is None or field(pull, 'merge_commit_sha') != workflow_sha
                    or field(pull, 'base', 'ref') != branch
                    or field(pull, 'base', 'repo', 'id') != repo_id
                    or field(pull, 'base', 'repo', 'full_name') != repository):
                continue
            number = field(pull, 'number')
            head = field(pull, 'head', 'sha')
            require(selected is None and positive(number) and sha(head))
            selected = (number, head)
    require(selected is not None)
    commit = api.get(f'{prefix}/git/commits/{workflow_sha}')
    tree = field(commit, 'tree', 'sha')
    require(field(commit, 'sha') == workflow_sha and sha(tree))
    return (repo_id, selected[0], selected[1], tree)


def utc(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z', value) is not None


def reviewed_source(api, request, reviewed, required_check):
    repository = request['repository']
    branch = request['default_branch']
    workflow_sha = request['workflow_sha']
    repo_id, number, head, tree = reviewed
    prefix = f'repos/{repository}'
    branch_name = quote(branch, safe='')

    def observe():
        repo = api.get(prefix)
        pull = api.get(f'{prefix}/pulls/{number}')
        require(field(repo, 'id') == repo_id and field(repo, 'full_name') == repository
                and field(repo, 'default_branch') == branch)
        require(field(pull, 'number') == number and field(pull, 'head', 'sha') == head
                and field(pull, 'head', 'repo', 'id') == repo_id
                and field(pull, 'head', 'repo', 'full_name') == repository
                and field(pull, 'base', 'repo', 'id') == repo_id
                and field(pull, 'base', 'repo', 'full_name') == repository
                and field(pull, 'base', 'ref') == branch and field(pull, 'state') == 'closed'
                and field(pull, 'merged') is True and field(pull, 'merged_at') is not None
                and field(pull, 'merge_commit_sha') == workflow_sha)
        rules = [item for batch in pages(api, f'{prefix}/rules/branches/{branch_name}') for item in batch]
        require(any(field(rule, 'type') == 'required_status_checks'
                    and isinstance(field(rule, 'parameters', 'required_status_checks'), list)
                    and any(field(check, 'context') == 'Required' for check in rule['parameters']['required_status_checks'])
                    for rule in rules))
        require(any(field(rule, 'type') == 'pull_request'
                    and field(rule, 'parameters', 'required_review_thread_resolution') is True for rule in rules))
        head_check = required_check(api, prefix, head, request['required_workflow_path'], None)
        commit = api.get(f'{prefix}/git/commits/{workflow_sha}')
        comparison = api.get(f'{prefix}/compare/{workflow_sha}...{branch_name}')
        require(field(commit, 'sha') == workflow_sha and field(commit, 'tree', 'sha') == tree
                and field(comparison, 'status') in ('ahead', 'identical'))
        merge_check = required_check(api, prefix, workflow_sha, request['required_workflow_path'], None)
        return (repo, pull, rules, head_check, commit, comparison, merge_check)

    first = observe()
    require(first == observe())
    return first


def reviews(api, request, reviewed, pull):
    repo_id, number, head, _tree = reviewed
    del repo_id
    prefix = f'repos/{request["repository"]}'
    author_id = field(pull, 'user', 'id')
    author_login = field(pull, 'user', 'login')
    merged = field(pull, 'merged_at')
    require(positive(author_id) and isinstance(author_login, str) and author_login and utc(merged))
    latest = {}
    ambiguous = set()
    seen = set()
    observed = []
    actor_types = {}
    for batch in pages(api, f'{prefix}/pulls/{number}/reviews'):
        for review in batch:
            observed.append(review)
            review_id = field(review, 'id')
            require(positive(review_id) and review_id not in seen)
            seen.add(review_id)
            state = field(review, 'state')
            if state in ('PENDING', 'COMMENTED'):
                continue
            actor = field(review, 'user')
            actor_id = field(actor, 'id')
            login = field(actor, 'login')
            kind = field(actor, 'type')
            require(positive(actor_id) and isinstance(login, str) and kind in ('User', 'Bot'))
            if (actor_id == author_id or login.lower() == author_login.lower()
                    or field(review, 'author_association') not in ('OWNER', 'MEMBER', 'COLLABORATOR')):
                continue
            require(actor_id not in actor_types or actor_types[actor_id] == kind)
            actor_types[actor_id] = kind
            permission = api.get(f'{prefix}/collaborators/{quote(login, safe="")}/permission')
            observed.append(permission)
            require(field(permission, 'user', 'id') == actor_id and field(permission, 'user', 'type') == kind)
            if field(permission, 'permission') not in ('write', 'admin'):
                continue
            submitted = field(review, 'submitted_at')
            require(state in ('APPROVED', 'CHANGES_REQUESTED', 'DISMISSED') and utc(submitted))
            if submitted > merged:
                continue
            require(submitted != merged)
            previous = latest.get(actor_id)
            if previous is not None and previous[0] == submitted:
                ambiguous.add(actor_id)
                continue
            if previous is not None and previous[0] > submitted:
                continue
            ambiguous.discard(actor_id)
            latest[actor_id] = (submitted, state, field(review, 'commit_id'), kind)
    require(not ambiguous)
    require(not any(item[1] == 'CHANGES_REQUESTED' for item in latest.values()))
    require(any(item[1] == 'APPROVED' and item[2] == head and item[3] == 'User' for item in latest.values()))
    return (latest, observed)


def workflow_suites(api, prefix, source, workflow_path):
    suites = {}
    for batch in pages(api, f'{prefix}/actions/runs?head_sha={source}', 'workflow_runs'):
        for run in batch:
            path = field(run, 'path')
            require(isinstance(path, str) and path and field(run, 'head_sha') == source)
            if path == workflow_path:
                suite = field(run, 'check_suite_id')
                require(positive(suite) and suite not in suites)
                suites[suite] = run
    return suites


def required_check(api, prefix, source, workflow_path, branch):
    suites = workflow_suites(api, prefix, source, workflow_path)
    candidates = []
    numbers = set()
    for suite, run in suites.items():
        if branch is not None and (field(run, 'event') != 'push' or field(run, 'head_branch') != branch):
            continue
        number, attempt, run_id = (field(run, name) for name in ('run_number', 'run_attempt', 'id'))
        require(positive(number) and positive(attempt) and positive(run_id) and number not in numbers)
        numbers.add(number)
        candidates.append((number, attempt, run_id, suite, run))
    require(bool(candidates))
    _, attempt, run_id, suite, run = max(candidates, key=lambda item: item[0])
    require(field(run, 'status') == 'completed' and field(run, 'conclusion') == 'success')
    job_id = None
    for batch in pages(api, f'{prefix}/actions/runs/{run_id}/attempts/{attempt}/jobs', 'jobs'):
        for job in batch:
            if field(job, 'name') != 'Required':
                continue
            selected = field(job, 'id')
            require(job_id is None and positive(selected) and field(job, 'run_id') == run_id
                    and field(job, 'run_attempt') == attempt
                    and field(job, 'check_run_url') == f'https://api.github.com/{prefix}/check-runs/{selected}'
                    and field(job, 'status') == 'completed' and field(job, 'conclusion') == 'success')
            job_id = selected
    require(job_id is not None)
    found = None
    seen = set()
    for batch in pages(api, f'{prefix}/commits/{source}/check-runs?filter=all', 'check_runs'):
        for check in batch:
            check_id = field(check, 'id')
            require(positive(check_id) and check_id not in seen)
            seen.add(check_id)
            if check_id != job_id or field(check, 'name') != 'Required' or field(check, 'check_suite', 'id') != suite:
                continue
            require(field(check, 'head_sha') == source and field(check, 'app', 'id') == 15368
                    and field(check, 'app', 'slug') == 'github-actions' and utc(field(check, 'started_at'))
                    and field(check, 'status') == 'completed' and field(check, 'conclusion') == 'success')
            found = check_id
    require(found is not None and workflow_suites(api, prefix, source, workflow_path) == suites)
    if branch is not None:
        repository = api.get(prefix)
        name = prefix.removeprefix('repos/')
        owner = name.split('/')[0]
        repo_id = field(repository, 'id')
        owner_id = field(repository, 'owner', 'id')
        require(positive(repo_id) and positive(owner_id) and field(repository, 'full_name') == name
                and field(repository, 'owner', 'login') == owner and field(repository, 'default_branch') == branch)
        for key in ('repository', 'head_repository'):
            require(field(run, key, 'id') == repo_id and field(run, key, 'full_name') == name
                    and field(run, key, 'owner', 'id') == owner_id and field(run, key, 'owner', 'login') == owner)
    matched = []
    for batch in pages(api, f'{prefix}/actions/runs?head_sha={source}', 'workflow_runs'):
        matched.extend(item for item in batch if field(item, 'check_suite_id') == suite)
    require(len(matched) == 1 and matched[0] == run and field(run, 'path') == workflow_path
            and field(run, 'head_sha') == source)
    observed = workflow_suites(api, prefix, source, workflow_path)
    require(observed == suites)
    return found


def repository_identity(api, request):
    name = request['repository']
    owner = name.split('/')[0]
    prefix = f'repos/{name}'
    repo = api.get(prefix)
    run = api.get(f'{prefix}/actions/runs/{request["run_id"]}')
    repo_id = field(repo, 'id')
    owner_id = field(repo, 'owner', 'id')
    require(positive(repo_id) and positive(owner_id) and field(repo, 'full_name') == name
            and field(repo, 'owner', 'login') == owner and field(run, 'id') == request['run_id']
            and field(run, 'run_attempt') == request['run_attempt'])
    for key in ('repository', 'head_repository'):
        require(field(run, key, 'id') == repo_id and field(run, key, 'full_name') == name
                and field(run, key, 'owner', 'id') == owner_id and field(run, key, 'owner', 'login') == owner)
    return (repo_id, owner_id)


def run_authority(api, request):
    name = request['repository']
    branch = request['default_branch']
    prefix = f'repos/{name}'
    repo = api.get(prefix)
    run = api.get(f'{prefix}/actions/runs/{request["run_id"]}')
    require(field(repo, 'full_name') == name and field(repo, 'default_branch') == branch
            and field(run, 'id') == request['run_id'] and field(run, 'run_attempt') == request['run_attempt']
            and field(run, 'head_sha') == request['workflow_sha'] and field(run, 'head_branch') == branch
            and field(run, 'event') == 'workflow_dispatch' and field(run, 'path') == '.github/workflows/release.yml'
            and field(run, 'repository', 'full_name') == name and field(run, 'head_repository', 'full_name') == name)
    source = request['workflow_sha']
    require(field(api.get(f'{prefix}/compare/{source}...{source}'), 'status') in ('ahead', 'identical'))
    require(field(api.get(f'{prefix}/compare/{source}...{quote(branch, safe="")}'), 'status') in ('ahead', 'identical'))


def environment(api, request):
    prefix = f'repos/{request["repository"]}'
    env_name = request['environment']
    encoded = quote(env_name, safe='')
    value = api.get(f'{prefix}/environments/{encoded}')
    env_id = field(value, 'id')
    name = field(value, 'name')
    require(isinstance(name, str) and name.lower() == env_name.lower() and positive(env_id)
            and field(value, 'deployment_branch_policy', 'custom_branch_policies') is True
            and field(value, 'can_admins_bypass') is False)
    protections = field(value, 'protection_rules')
    require(isinstance(protections, list))
    reviewers = [rule for rule in protections if field(rule, 'type') == 'required_reviewers']
    require(len(reviewers) == 1)
    allowed = field(reviewers[0], 'reviewers')
    require(isinstance(allowed, list) and len(allowed) == 1 and field(allowed[0], 'type') == 'User'
            and field(allowed[0], 'reviewer', 'id') == request['reviewer_id'])
    policies = api.get(f'{prefix}/environments/{encoded}/deployment-branch-policies?per_page=100')
    rules = field(policies, 'branch_policies')
    require(field(policies, 'total_count') == 1 and isinstance(rules, list) and len(rules) == 1
            and field(rules[0], 'type') == 'branch' and field(rules[0], 'name') == request['default_branch'])
    return (env_id, value, policies)


def approval(api, request):
    require(request['run_attempt'] == 1)
    run_authority(api, request)
    env_id, env_value, policies = environment(api, request)
    prefix = f'repos/{request["repository"]}'
    data = api.get(f'{prefix}/actions/runs/{request["run_id"]}/approvals')
    require(isinstance(data, list))
    relevant = []
    for item in data:
        envs = field(item, 'environments')
        if isinstance(envs, list) and any(field(env, 'id') == env_id
                and isinstance(field(env, 'name'), str)
                and field(env, 'name').lower() == request['environment'].lower() for env in envs):
            relevant.append(item)
    require(len(relevant) == 1)
    expected = (f'I reviewed the complete release policy at {request["workflow_sha"]} and source '
                f'{request["workflow_sha"]}, and authorize this source for run {request["run_id"]}.')
    item = relevant[0]
    require(field(item, 'state') == 'approved' and field(item, 'user', 'type') == 'User'
            and field(item, 'user', 'id') == request['reviewer_id'] and field(item, 'comment') == expected)
    return (env_value, policies, data)


def admitted(api, request):
    require(request['run_attempt'] == 1 and positive(request['run_id']) and positive(request['reviewer_id']))
    reviewed = reviewed_merge(api, request)
    first = reviewed_source(api, request, reviewed, required_check)
    pull = api.get(f'repos/{request["repository"]}/pulls/{reviewed[1]}')
    review_evidence = reviews(api, request, reviewed, pull)
    identity = repository_identity(api, request)
    run_authority(api, request)
    environment(api, request)
    check = required_check(api, f'repos/{request["repository"]}', request['workflow_sha'],
                           request['required_workflow_path'], request['default_branch'])
    approval_evidence = None
    for _ in range(2):
        require(repository_identity(api, request) == identity)
        require(required_check(api, f'repos/{request["repository"]}', request['workflow_sha'],
                               request['required_workflow_path'], request['default_branch']) == check)
        observed_approval = approval(api, request)
        require(approval_evidence is None or observed_approval == approval_evidence)
        approval_evidence = observed_approval
        require(repository_identity(api, request) == identity)
        if _ == 0:
            require(reviewed_source(api, request, reviewed, required_check) == first
                    and api.get(f'repos/{request["repository"]}/pulls/{reviewed[1]}') == pull)
            require(reviews(api, request, reviewed, pull) == review_evidence)
    require(reviewed_merge(api, request) == reviewed and identity[0] == reviewed[0])
    return (identity, approval_evidence, review_evidence, reviewed, first, pull, check)


def context(environ):
    names = ('GITHUB_REPOSITORY', 'VELNOR_QUALIFICATION_DEFAULT_BRANCH', 'VELNOR_QUALIFICATION_ENVIRONMENT',
             'VELNOR_QUALIFICATION_REQUIRED_WORKFLOW', 'GITHUB_SHA', 'GITHUB_EVENT_NAME',
             'VELNOR_QUALIFICATION_MODE', 'GITHUB_REF', 'GITHUB_WORKFLOW_SHA', 'GITHUB_WORKFLOW_REF', 'GITHUB_JOB',
             'VELNOR_QUALIFICATION_REVIEWER_ID', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')
    require(all(isinstance(environ.get(name), str) and environ[name] for name in names))
    repository = environ['GITHUB_REPOSITORY']
    branch = environ['VELNOR_QUALIFICATION_DEFAULT_BRANCH']
    workflow_sha = environ['GITHUB_SHA']
    reviewer = environ['VELNOR_QUALIFICATION_REVIEWER_ID']
    run_id = environ['GITHUB_RUN_ID']
    attempt = environ['GITHUB_RUN_ATTEMPT']
    require(all(re.fullmatch(r'[1-9][0-9]*', item) for item in (reviewer, run_id, attempt)))
    require(environ['GITHUB_EVENT_NAME'] == 'workflow_dispatch'
            and environ['VELNOR_QUALIFICATION_MODE'] == 'oidc-qualification'
            and environ['GITHUB_JOB'] == 'oidc_qualification'
            and environ['GITHUB_REF'] == f'refs/heads/{branch}'
            and environ['GITHUB_WORKFLOW_SHA'] == workflow_sha
            and environ['GITHUB_WORKFLOW_REF'] == f'{repository}/.github/workflows/release.yml@refs/heads/{branch}')
    required_path = environ['VELNOR_QUALIFICATION_REQUIRED_WORKFLOW']
    require(re.fullmatch(r'\.github/workflows/(?!\.)[A-Za-z0-9_.-]+\.(?:yml|yaml)', required_path) is not None)
    env_name = environ['VELNOR_QUALIFICATION_ENVIRONMENT']
    require(len(env_name) <= 255 and re.fullmatch(r'[A-Za-z0-9_./-]+', env_name) is not None
            and all(segment not in ('', '.', '..') for segment in env_name.split('/')))
    return {'repository': repository, 'default_branch': branch, 'environment': env_name,
            'required_workflow_path': required_path, 'workflow_sha': workflow_sha,
            'reviewer_id': int(reviewer), 'run_id': int(run_id), 'run_attempt': int(attempt)}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        if key in ('id', 'number', 'run_id', 'run_attempt', 'run_number', 'check_suite_id', 'total_count'):
            require(value is None or (type(value) is int and value >= 0))
        result[key] = value
    return result


def unique_claim_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def reject_constant(_value):
    raise AdmissionError('OIDC qualification API response invalid')


class GithubAPI:
    def __init__(self, token, opener):
        require(isinstance(token, str) and token and '\r' not in token and '\n' not in token)
        self.token = token
        self.opener = opener

    def get(self, path):
        import json
        import urllib.request
        require(isinstance(path, str) and path.startswith('repos/') and not path.startswith('repos//')
                and '\r' not in path and '\n' not in path and '#' not in path)
        request = urllib.request.Request('https://api.github.com/' + path, method='GET', headers={
            'Authorization': 'Bearer ' + self.token,
            'Accept': 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'velnor-oidc-qualification',
        })
        with self.opener.open(request, timeout=15) as response:
            require(response.status == 200)
            data = response.read(2 * 1024 * 1024 + 1)
            require(len(data) <= 2 * 1024 * 1024)
        try:
            return json.loads(data, object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (TypeError, UnicodeDecodeError, ValueError) as error:
            raise AdmissionError('OIDC qualification API response invalid') from error


def run_qualification(environ, github_opener, oidc_opener):
    request_context = context(environ)
    api = GithubAPI(environ.get('GIT_TOKEN'), github_opener)
    admission_evidence = admitted(api, request_context)
    identity = admission_evidence[0]
    oidc_url = environ.get('ACTIONS_ID_TOKEN_REQUEST_URL')
    oidc_bearer = environ.get('ACTIONS_ID_TOKEN_REQUEST_TOKEN')
    require(isinstance(oidc_url, str) and valid_request_url(oidc_url)
            and isinstance(oidc_bearer, str) and 0 < len(oidc_bearer) <= MAX_RESPONSE
            and '\r' not in oidc_bearer and '\n' not in oidc_bearer)
    bound_exchange(api, request_context, admission_evidence, oidc_opener, oidc_url, oidc_bearer)


def qualification_main():
    import os
    import ssl
    import sys
    import urllib.request
    try:
        require('SSLKEYLOGFILE' not in os.environ)
        tls = ssl.create_default_context(cafile='/etc/ssl/certs/ca-certificates.crt')
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), RejectRedirect(),
                                             urllib.request.HTTPSHandler(context=tls))
        run_qualification(os.environ, opener, opener)
    except Exception:
        print('OIDC qualification failed', file=sys.stderr)
        return 1
    print('OIDC qualification succeeded')
    return 0


def claim_number(value):
    if type(value) is int:
        return value if value > 0 else None
    if isinstance(value, str) and re.fullmatch(r'[1-9][0-9]*', value):
        return int(value)
    return None


def bound_exchange(api, context_value, admission_evidence, opener, oidc_url, oidc_bearer):
    identity = admission_evidence[0]
    import base64
    import binascii
    import json
    import urllib.parse
    separator = '&' if urllib.parse.urlsplit(oidc_url).query else '?'
    response = request(opener, oidc_url + separator + 'audience=crates.io', 'GET',
                       {'Authorization': 'Bearer ' + oidc_bearer})
    try:
        value = json.loads(response, object_pairs_hook=unique_object, parse_constant=reject_constant)
        token = value['value']
        require(isinstance(token, str) and 0 < len(token) <= MAX_RESPONSE)
        parts = token.split('.')
        require(len(parts) == 3 and all(parts))
        payload = base64.b64decode(parts[1] + '=' * (-len(parts[1]) % 4), altchars=b'-_', validate=True)
        require(len(payload) <= MAX_RESPONSE)
        claims = json.loads(payload, object_pairs_hook=unique_claim_object, parse_constant=reject_constant)
    except (KeyError, ValueError, TypeError, UnicodeDecodeError, binascii.Error) as error:
        raise AdmissionError('OIDC identity response invalid') from error
    name = context_value['repository']
    branch = context_value['default_branch']
    source = context_value['workflow_sha']
    require(field(claims, 'iss') == 'https://token.actions.githubusercontent.com'
            and field(claims, 'aud') == 'crates.io' and field(claims, 'repository') == name
            and claim_number(field(claims, 'repository_id')) == identity[0]
            and claim_number(field(claims, 'repository_owner_id')) == identity[1]
            and claim_number(field(claims, 'run_id')) == context_value['run_id']
            and claim_number(field(claims, 'run_attempt')) == context_value['run_attempt']
            and field(claims, 'environment') == context_value['environment']
            and field(claims, 'event_name') == 'workflow_dispatch'
            and field(claims, 'ref') == f'refs/heads/{branch}'
            and field(claims, 'sha') == source and field(claims, 'workflow_sha') == source
            and field(claims, 'workflow_ref') == f'{name}/.github/workflows/release.yml@refs/heads/{branch}'
            and field(claims, 'runner_environment') == 'github-hosted')
    job_id = claim_number(field(claims, 'check_run_id'))
    require(job_id is not None)
    job = api.get(f'repos/{name}/actions/jobs/{job_id}')
    require(field(job, 'id') == job_id and field(job, 'run_id') == context_value['run_id']
            and field(job, 'run_attempt') == context_value['run_attempt']
            and field(job, 'head_sha') == source and field(job, 'name') == 'OIDC qualification'
            and field(job, 'status') == 'in_progress')
    require(admitted(api, context_value) == admission_evidence)
    require(api.get(f'repos/{name}/actions/jobs/{job_id}') == job)
    data = request(opener, REGISTRY, 'POST',
                   {'Content-Type': 'application/json', 'User-Agent': 'velnor-oidc-qualification'},
                   json.dumps({'jwt': token}, separators=(',', ':')).encode())
    try:
        credential = json.loads(data, object_pairs_hook=unique_object, parse_constant=reject_constant)['token']
        require(isinstance(credential, str) and 0 < len(credential) <= MAX_RESPONSE)
    except (KeyError, ValueError, TypeError, UnicodeDecodeError) as error:
        raise AdmissionError('OIDC credential response invalid') from error
    request(opener, REGISTRY, 'DELETE',
            {'Authorization': 'Bearer ' + credential, 'User-Agent': 'velnor-oidc-qualification'})
