#!/usr/bin/env bash
# Architecture guard: the inner crates must NEVER depend on `tauri`. The campaign
# engine and the domain/persistence/cloud layers stay out of the Tauri/wry blast
# radius — they reach the UI only through the bigbox-contract ports. A PR that
# adds `tauri` to any crate below is wrong by construction (the logic belongs one
# layer up, in bigbox-shell / bigbox-vorcaro / the app, behind a port).
#
# See docs/WORKSPACE-REFACTOR-PLAN.md §7.
set -euo pipefail

# Tauri may appear ONLY in: bigbox-shell, bigbox-vorcaro, bigbox (the app).
# Everything else must be tauri-free. (bigbox-drivers is added here once the
# typed driver layer lands — see plan §8 step 7.)
INNER_CRATES=(
  bigbox-core
  bigbox-contract
  bigbox-driver-assets
  bigbox-config
  bigbox-cloud
  bigbox-orchestrator
)

fail=0
for crate in "${INNER_CRATES[@]}"; do
  # Strip the ` (/abs/path)` parenthetical cargo tree prints for every path
  # dependency — this repo lives in .../BigBox-Tauri, which would otherwise
  # false-match. After stripping, a line contains "tauri" only if the `tauri`
  # crate (or a `tauri-*` crate) is an actual dependency.
  if cargo tree -p "$crate" -e normal 2>/dev/null \
      | sed -E 's/ \([^)]*\)//g' \
      | grep -qi 'tauri'; then
    echo "❌ $crate depends on tauri — move the logic up a layer, behind a port."
    fail=1
  else
    echo "✅ $crate is tauri-free"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo
  echo "Architecture guard failed: tauri leaked into an inner crate."
  exit 1
fi
echo
echo "Architecture guard passed: all inner crates are tauri-free."
