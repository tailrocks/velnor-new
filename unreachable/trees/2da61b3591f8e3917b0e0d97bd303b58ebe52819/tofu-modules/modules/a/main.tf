module "nested" {
  source = "./nested"
}
module "shared" {
  source = "../shared"
}
output "nested_o" {
  value = module.nested.o
}
output "shared_o" {
  value = module.shared.o
}
