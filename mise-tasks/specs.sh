#!/usr/bin/env bash
#MISE description="Validate every OpenSpec change and spec"
# OpenSpec's standalone release binary; it needs no node.
#MISE tools = { "github:Fission-AI/OpenSpec" = "1.16.0" }
openspec validate --all --strict --no-interactive
