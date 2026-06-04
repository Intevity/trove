provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project   = "trove"
      Component = "auto-update-channel"
      ManagedBy = "terraform"
      Repo      = "${var.github_owner}/${var.github_repo}"
    }
  }
}
