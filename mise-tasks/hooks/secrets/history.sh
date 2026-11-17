#!/usr/bin/env bash
#MISE description="Scan the history for credentials; arguments such as --log-opts=<range> narrow it"
# gitleaks comes from mise.dev.toml. The CI scan passes a pull request's
# commits, or nothing for the whole history. hooks:pushed runs gitleaks with
# these flags itself, so a push starts no second mise process.
gitleaks git --redact --no-banner "$@"
