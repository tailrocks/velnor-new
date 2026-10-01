# OFFLINE FIXTURE: init requires network; never init in tests.
module "git" {
  source = "git::https://example.com/acme/widgets.git?ref=v1.0.0"
}
module "reg" {
  source  = "example-ns/widgets/fictional"
  version = "~> 1.0"
}
