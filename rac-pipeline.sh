#!/bin/bash
# rac-pipeline.sh — Run the full extraction pipeline from the rac_tools project root
cd "$(dirname "$0")" || exit 1
exec ./target/release/rac_tools pipeline "$@"
