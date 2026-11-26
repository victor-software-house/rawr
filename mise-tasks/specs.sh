#!/usr/bin/env bash
#MISE description="Validate every OpenSpec change and spec"
# This task runs the release binary of OpenSpec. It needs no node.
#MISE tools = { "github:Fission-AI/OpenSpec" = "2.2.0" }
openspec validate --all --strict --no-interactive
