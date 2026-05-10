#!/bin/bash
# rac-viewer.sh — Launch the 3D viewer from the rac_tools project root
cd "$(dirname "$0")" || exit 1
exec ./target/release/rac_tools viewer "$@"
