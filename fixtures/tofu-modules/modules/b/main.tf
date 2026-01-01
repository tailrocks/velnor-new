module "shared" {
  source = "../shared"
}
output "shared_o" {
  value = module.shared.o
}
