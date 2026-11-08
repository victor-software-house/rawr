#!/usr/bin/env bash
#MISE description="Scan the staged diff for credentials"
# gitleaks comes from mise.dev.toml.
gitleaks git --staged --redact --no-banner
