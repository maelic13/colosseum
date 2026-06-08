#!/usr/bin/env bash
# Build the release binary and copy it into dist/.
#
# The binary is fully self-contained (fonts and icon are embedded); the file
# in dist/ is the whole distributable. Desktop-menu integration (icon, launcher
# entry) is handled separately by the Flatpak packaging in flatpak/.
#
# Usage:  ./build_linux.sh
# Output: dist/colosseum

set -euo pipefail

cd "$(dirname "$0")"

cargo build --release --bin colosseum

mkdir -p dist
cp target/release/colosseum dist/colosseum

echo "Built dist/colosseum"
