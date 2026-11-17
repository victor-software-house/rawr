#!/usr/bin/env bash
#MISE description="Lint one commit or range with committed"
# A by-hand check; the pre-push hook runs committed itself.
committed "${1:-HEAD}"
