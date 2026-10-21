#!/usr/bin/env bash
# Compatibility entry point; shared implementation owns paths and feature mapping.
set -euo pipefail
exec bash "$(dirname -- "${BASH_SOURCE[0]}")/scripts/run-gpu.sh" dev
