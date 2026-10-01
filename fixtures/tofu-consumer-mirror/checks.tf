check "env_set" {
  assert {
    condition     = var.env != ""
    error_message = "env empty"
  }
}
check "org_set" {
  assert {
    condition     = local.org != ""
    error_message = "org empty"
  }
}
check "repos_known" {
  assert {
    condition     = length(local.repo_names) > 0
    error_message = "no repos"
  }
}
check "mirror_on" {
  assert {
    condition     = local.mirror
    error_message = "mirror off"
  }
}
check "policy_set" {
  assert {
    condition     = module.repository_policy.policy_id != ""
    error_message = "policy empty"
  }
}
