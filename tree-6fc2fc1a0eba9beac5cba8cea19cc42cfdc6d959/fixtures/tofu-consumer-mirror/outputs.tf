output "policy" {
  value = module.repository_policy.policy_id
}
output "repos" {
  value = var.runner_group_repos
}
