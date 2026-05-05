#!/bin/bash
set -e -o pipefail

ACCOUNT_ID="A-..."
IMPORT_MPAN="..."
EXPORT_MPAN="..."
SERIAL="..."

. <(pass "octopus.energy/env")

cargo run --bin save_consumption_data -- \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${IMPORT_MPAN}" \
    --serial "${SERIAL}" \
    data/power.sqlite3

cargo run --bin save_consumption_data -- \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${EXPORT_MPAN}" \
    --serial "${SERIAL}" \
    data/power.sqlite3

./scripts/version_db.sh
