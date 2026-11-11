#!/usr/bin/env bash
#MISE description="Scan the history for credentials; arguments such as --log-opts=<range> narrow it"
# gitleaks comes from mise.dev.toml. The pre-push hook passes the pushed
# commits; the CI scan passes a pull request's commits, or nothing for the
# whole history.
gitleaks git --redact --no-banner "$@"
