#!/bin/bash
set -e -o pipefail

cargo run --bin save_consumption_data -- \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${IMPORT_MPAN}" \
    --serial "${SERIAL}" \
    --type "import" \
    --force true \
    data/power.sqlite3

cargo run --bin save_consumption_data -- \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${EXPORT_MPAN}" \
    --serial "${SERIAL}" \
    --type "export" \
    --force true \
    data/power.sqlite3

./scripts/version_db.sh
