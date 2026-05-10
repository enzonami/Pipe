#!/bin/bash
# rac-iso-pack.sh — Build repacked ISO from repacked WADs
cd "$(dirname "$0")" || exit 1
exec ./target/release/rac_tools iso-pack "$@"
