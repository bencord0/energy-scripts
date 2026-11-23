#!/bin/bash
set -e -o pipefail

# Dispatches re-price slots to off-peak
cargo_run save_dispatches \
    --account-id "${ACCOUNT_ID}" \
    data/power.sqlite3
