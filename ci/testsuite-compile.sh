#!/usr/bin/env bash
set -euo pipefail

echo "Running testsuite compile check"

# https://stackoverflow.com/a/246128
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"

cd "$SCRIPT_DIR/.."

for testsuite in *-testsuite
do
    cargo test \
        -p "$testsuite" \
        --target thumbv7em-none-eabi \
        --no-run
done
