#!/bin/bash
# Build DDR texture pack from a directory of PNGs
# Usage: ./scripts/build_ddr_package.sh <input_dir> [output_name]
#
# Examples:
#   ./scripts/build_ddr_package.sh assets/my_textures
#   ./scripts/build_ddr_package.sh assets/my_textures my_mod

set -e
cd "$(dirname "$0")/.."

INPUT_DIR="${1:?Usage: $0 <input_dir> [output_name]}"
OUTPUT_NAME="${2:-custom_mod}"

python3 -m scripts.build_ddr_package "$INPUT_DIR" -o "mod_assets/${OUTPUT_NAME}.arc" --name "$OUTPUT_NAME"

echo ""
echo "Output: mod_assets/${OUTPUT_NAME}.arc"
