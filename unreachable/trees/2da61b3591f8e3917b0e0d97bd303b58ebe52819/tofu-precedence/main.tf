# Shadowed by main.tofu (same basename): discovery must exclude this file
# from the load set. The duplicate "which" below must NOT error.
variable "which" {
  default = "tf"
}
variable "ghost" {
  default = "only-in-tf"
}
