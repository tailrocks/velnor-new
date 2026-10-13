"""Exact reviewed live protection constraints for source-only release publication.

Measured 2026-10-03; requires live rereads and changes no repository settings.
Main checks may fail: this policy never claims runtime or CI qualification.
"""

import json
import re

from source_publication import REPO, require

MAIN_ID = 24396608
TAG_ID = 24397132
MAIN_RECEIPT_SHA256 = "752c64ca98d85f2fbfcbb6ebeac3b853892d4b4fd5b72a064dc8b24a671eebcf"
TAG_RECEIPT_SHA256 = "fc0d3f81246c7fb701be2f1f02a4a515c347a0fe5d48212697f07277ef867a4c"
COMMON = {"source_type": "Repository", "source": REPO, "enforcement": "active",
          "bypass_actors": [], "current_user_can_bypass": "never"}
MAIN = {**COMMON, "id": MAIN_ID, "name": "protect-main", "target": "branch",
    "conditions": {"ref_name": {"exclude": [], "include": ["~DEFAULT_BRANCH"]}},
    "rules": [{"type": "deletion"}, {"type": "non_fast_forward"},
        {"type": "required_linear_history"}, {"type": "pull_request", "parameters": {
            "required_approving_review_count": 0, "dismiss_stale_reviews_on_push": True,
            "required_reviewers": [], "require_code_owner_review": False,
            "dismissal_restriction": {"enabled": False, "allowed_actors": []},
            "require_last_push_approval": False, "required_review_thread_resolution": True,
            "require_extra_approval_for_unattributed_changes": True,
            "allowed_merge_methods": ["squash"]}},
        {"type": "required_status_checks", "parameters": {
            "strict_required_status_checks_policy": True, "do_not_enforce_on_create": False,
            "required_status_checks": [{"context": "Required"}]}}]}
TAGS = {**COMMON, "id": TAG_ID, "name": "protect-tags", "target": "tag",
    "conditions": {"ref_name": {"exclude": [], "include": ["~ALL"]}},
    "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}


def reviewed_ruleset(actual, expected):
    require(isinstance(actual, dict), "protection ruleset response must be an object")
    measured = {key: actual.get(key) for key in expected}
    require(json.dumps(measured, sort_keys=True) == json.dumps(expected, sort_keys=True),
            "live protection constraints differ from reviewed ruleset")


def verify_protection(api, tag):
    require(re.fullmatch(r"owned-source-(?:mise|mbx-action)-[0-9a-f]{40}", tag),
            "unreviewed source tag shape")
    require(api("").get("default_branch") == "main", "default branch changed")
    reviewed_ruleset(api("rulesets/" + str(MAIN_ID)), MAIN)
    reviewed_ruleset(api("rulesets/" + str(TAG_ID)), TAGS)
