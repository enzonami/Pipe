#!/bin/bash
# rac-repack.sh — Recompress unpacked .bin data and rebuild level WADs
cd "$(dirname "$0")" || exit 1
exec ./target/release/rac_tools wad-repack "$@"
