# Terraform: Trove legacy auto-update bridge

> **Transitional.** As of v0.8.6 the update channel is the GitHub release itself —
> installed apps poll `releases/latest/download/latest.json`. This bucket now serves a
> single file, `stable/latest.json`, for binaries built **before** the switch, whose S3
> endpoint is baked in. Its per-platform URLs point at the GitHub release assets, so an
> old install upgrades itself onto the GitHub channel; bundles are no longer mirrored here.
> See [documentation/releasing.md](../documentation/releasing.md#the-s3-bridge-transitional).
>
> To retire the whole stack once legacy installs have aged out:
> `gh variable delete S3_BUCKET --repo Intevity/trove` (the `bridge-s3` job then skips),
> then `terraform destroy`.

Provisions the public S3 bucket plus the GitHub Actions **OIDC role** the release workflow
assumes to publish to it. No long-lived AWS keys are created.

## What it creates

- **S3 bucket** (`intevity-trove-updates` by default) — ACLs disabled
  (`BucketOwnerEnforced`), SSE-S3, with a bucket policy granting **anonymous `s3:GetObject`
  on the `stable/*` prefix only**. The compiled binaries are public; nothing else in the
  bucket is, and the source repo stays private.
- **IAM role** (`trove-updates-publisher`) — assumable only via GitHub OIDC from
  this repo's **tag** pushes, with `s3:PutObject` on `stable/*` and nothing more.

State is stored in the shared **`intevity-si`** bucket under
`trove/updates/terraform.tfstate`, locked with S3-native locking
(`use_lockfile`, no DynamoDB table).

## Prerequisites

- Terraform **>= 1.10** (for `use_lockfile`).
- AWS credentials for an account principal allowed to create S3 buckets + IAM roles
  (e.g. `aws sso login` / an admin profile). Set `AWS_PROFILE`/`AWS_REGION` or use your
  usual auth before running.

## Usage

```sh
cd terraform
terraform init      # configures the intevity-si backend
terraform plan      # review
terraform apply
```

If `plan` errors that the GitHub OIDC provider does not exist, set
`create_github_oidc_provider = true` (in `terraform.tfvars`) and re-run.

## After apply

`terraform output` prints the four repo variables to set. The `gh_cli_setup` output is
copy/paste-ready:

```sh
terraform output -raw gh_cli_setup | sh   # or run the printed lines manually
```

That sets `S3_BUCKET`, `AWS_REGION`, `UPDATER_PUBLIC_BASE`, and `AWS_ROLE_ARN` as repo
**variables**. With OIDC there are **no** AWS secrets to store. Once set, the next `v*`
tag publishes signed updater artifacts to the bucket and the in-app updater goes live.

> Bucket names are global and must not contain dots (a dotted name breaks the
> virtual-hosted HTTPS endpoint the updater uses). Override `bucket_name` if the default
> is taken.
