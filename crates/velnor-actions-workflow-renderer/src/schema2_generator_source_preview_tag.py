# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

from datetime import datetime, timezone

OWNER_ID = 139017
OWNER_LOGIN = "donbeave"
CREATION_RULESET = 26696396
IMMUTABLE_TAG_RULESET = 24397132
PRECHECK_SECONDS = 900


def verify_tag_rules(client):
    path = "/repos/" + REPOSITORY + "/rulesets/"
    creation = client.get(path + str(CREATION_RULESET))
    frozen = client.get(path + str(IMMUTABLE_TAG_RULESET))
    for record in (creation, frozen):
        if record.get("target") != "tag" or record.get("enforcement") != "active":
            raise ValueError("SDK preview tag protection disabled")
    expected_actor = [{"actor_id": OWNER_ID, "actor_type": "User", "bypass_mode": "always"}]
    if (creation.get("id") != CREATION_RULESET
            or creation.get("bypass_actors") != expected_actor
            or creation.get("conditions", {}).get("ref_name") != {
                "include": ["refs/tags/sdk-preview-*"], "exclude": []}
            or {entry.get("type") for entry in creation.get("rules", [])} != {"creation"}):
        raise ValueError("SDK preview tag creation authority")
    if (frozen.get("id") != IMMUTABLE_TAG_RULESET or frozen.get("bypass_actors") != []
            or frozen.get("conditions", {}).get("ref_name") != {"include": ["~ALL"], "exclude": []}
            or not {"update", "deletion"}.issubset(
                {entry.get("type") for entry in frozen.get("rules", [])})):
        raise ValueError("SDK preview tag immutability")


def checked_time(value):
    if not isinstance(value, str) or not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", value):
        raise ValueError("SDK preview precheck timestamp")
    return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def annotation(message, source, tagger_date, now, version):
    if not isinstance(message, str) or len(message.encode()) > 8192:
        raise ValueError("SDK preview annotation size")
    record = json.loads(message, object_pairs_hook=unique_object)
    keys = {"schema", "repository", "artifact", "source_sha", "version", "targets", "immutability"}
    canonical = json.dumps(record, sort_keys=True, separators=(",", ":"))
    if set(record) != keys or message not in (canonical, canonical + "\n"):
        raise ValueError("SDK preview annotation shape")
    if (type(record["schema"]) is not int or record["schema"] != 2
            or record["repository"] != REPOSITORY or record["artifact"] != "generator"
            or record["source_sha"] != source or record["version"] != version):
        raise ValueError("SDK preview annotation identity")
    targets = record["targets"]
    if not isinstance(targets, dict) or set(targets) != {"linux", "darwin"}:
        raise ValueError("SDK preview annotated targets")
    ids = []
    for target in targets.values():
        if not isinstance(target, dict) or set(target) != {"producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256", "qualification_job_id"}:
            raise ValueError("SDK preview annotated producer")
        for key in ("producer_run_id", "producer_run_attempt", "payload_artifact_id", "qualification_job_id"):
            positive(target[key])
        if not re.fullmatch("[0-9a-f]{64}", target["payload_archive_sha256"]):
            raise ValueError("SDK preview annotated archive digest")
        ids.append(target["payload_artifact_id"])
    if len(set(ids)) != 2:
        raise ValueError("SDK preview annotated artifacts")
    precheck = record["immutability"]
    if not isinstance(precheck, dict) or set(precheck) != {"enabled", "checked_at"} or precheck["enabled"] is not True:
        raise ValueError("SDK preview owner immutability precheck")
    checked = checked_time(precheck["checked_at"])
    tag_time = datetime.fromisoformat(tagger_date.replace("Z", "+00:00"))
    if (tag_time.utcoffset() != timezone.utc.utcoffset(tag_time)
            or not 0 <= (tag_time - checked).total_seconds() <= 60
            or not 0 <= (now - checked).total_seconds() <= PRECHECK_SECONDS):
        raise ValueError("SDK preview owner precheck expired")
    return record


def tag_reference(client, source):
    tag = "sdk-preview-" + source
    reference = client.get("/repos/" + REPOSITORY + "/git/ref/tags/" + tag)
    target = reference.get("object", {})
    if (reference.get("ref") != "refs/tags/" + tag or target.get("type") != "tag"
            or not re.fullmatch("[0-9a-f]{40}", target.get("sha", ""))):
        raise ValueError("SDK preview annotated tag required")
    return target["sha"]


def verify_default_ancestry(client, source):
    main = client.get("/repos/" + REPOSITORY + "/git/ref/heads/main").get("object", {})
    if main.get("type") != "commit" or not re.fullmatch("[0-9a-f]{40}", main.get("sha", "")):
        raise ValueError("SDK preview protected main identity")
    comparison = client.get("/repos/" + REPOSITORY + "/compare/" + source + "..." + main["sha"])
    if (comparison.get("status") not in ("ahead", "identical")
            or comparison.get("merge_base_commit", {}).get("sha") != source):
        raise ValueError("SDK preview source outside protected main")


def admit_tag(client, source, version):
    tag = "sdk-preview-" + source
    expected_ref = REPOSITORY + "/" + WORKFLOW + "@refs/tags/" + tag
    if (not re.fullmatch("[0-9a-f]{40}", source)
            or os.environ.get("GITHUB_REPOSITORY") != REPOSITORY
            or os.environ.get("GITHUB_EVENT_NAME") != "push"
            or os.environ.get("GITHUB_REF") != "refs/tags/" + tag
            or os.environ.get("GITHUB_WORKFLOW_REF") != expected_ref
            or os.environ.get("GITHUB_WORKFLOW_SHA") != source
            or os.environ.get("GITHUB_ACTOR_ID") != str(OWNER_ID)
            or os.environ.get("GITHUB_TRIGGERING_ACTOR") != OWNER_LOGIN):
        raise ValueError("SDK preview tag push authority")
    owner = client.get("/users/" + OWNER_LOGIN)
    if owner.get("id") != OWNER_ID or owner.get("login") != OWNER_LOGIN:
        raise ValueError("SDK preview rerun authority")
    run_id = positive(int(os.environ["GITHUB_RUN_ID"]))
    attempt = positive(int(os.environ["GITHUB_RUN_ATTEMPT"]))
    run = client.get("/repos/" + REPOSITORY + "/actions/runs/" + str(run_id))
    if (run.get("id") != run_id or positive(run.get("run_attempt")) < attempt
            or run.get("head_sha") != source or run.get("head_branch") != tag
            or run.get("event") != "push" or run.get("path", "").split("@", 1)[0] != WORKFLOW
            or run.get("head_repository", {}).get("full_name") != REPOSITORY
            or run.get("head_repository", {}).get("id") != REPOSITORY_ID):
        raise ValueError("SDK preview tag run identity")
    now = checked_time(run.get("created_at"))
    verify_tag_rules(client)
    tag_sha = tag_reference(client, source)
    value = client.get("/repos/" + REPOSITORY + "/git/tags/" + tag_sha)
    if (value.get("sha") != tag_sha or value.get("tag") != tag
            or value.get("object") != {"type": "commit", "sha": source,
                                        "url": "https://api.github.com/repos/" + REPOSITORY + "/git/commits/" + source}):
        raise ValueError("SDK preview tag source")
    proof = annotation(value.get("message"), source, value.get("tagger", {}).get("date", ""), now, version)
    verify_default_ancestry(client, source)
    return proof, tag_sha, expected_ref
