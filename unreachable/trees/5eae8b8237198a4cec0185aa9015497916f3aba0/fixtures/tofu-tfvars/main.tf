variable "name" {
  default = "dflt"
}
variable "replicas" {
  default = 1
}
output "name" {
  value = "${var.name}-${var.replicas}"
}
