module "repository_policy" {
  source = "./modules/repository-policy"
}
moved {
  from = widgets_repo.legacy_a
  to   = module.repository_policy.widgets_repo.a
}
moved {
  from = widgets_repo.legacy_b
  to   = module.repository_policy.widgets_repo.b
}
moved {
  from = widgets_repo.legacy_c
  to   = module.repository_policy.widgets_repo.c
}
