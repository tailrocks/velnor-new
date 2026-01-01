# Inherits caller provider config (no provider block here).
terraform {
  required_version = ">= 1.5"
  required_providers {
    widgets = {
      source  = "example.com/acme/widgets"
      version = "~> 6.0"
    }
  }
}
resource "widgets_repo" "this" {
  name = var.repo_name
}
