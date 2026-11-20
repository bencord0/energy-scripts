#!/bin/bash
set -e -o pipefail

cargo run --bin save_promotion_events -- \
    --account-id "${ACCOUNT_ID}" \
    data/power.sqlite3
