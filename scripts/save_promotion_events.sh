#!/bin/bash
set -e -o pipefail

cargo_run save_promotion_events \
    --account-id "${ACCOUNT_ID}" \
    data/power.sqlite3
