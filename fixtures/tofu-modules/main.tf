module "a" {
  source = "./modules/a"
}
module "b" {
  source = "./modules/b"
}
output "a_nested" {
  value = module.a.nested_o
}
output "b_shared" {
  value = module.b.shared_o
}
