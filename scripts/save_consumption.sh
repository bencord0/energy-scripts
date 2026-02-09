#!/bin/bash
set -e -o pipefail

ACCOUNT_ID="A-..."
IMPORT_MPAN="..."
EXPORT_MPAN="..."
SERIAL="..."

. <(pass "octopus.energy/env")

python ./consumption.py --mpan "${IMPORT_MPAN}" --serial "${SERIAL}"
python ./consumption.py --mpan "${EXPORT_MPAN}" --serial "${SERIAL}"

python ./save_consumption_data.py \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${IMPORT_MPAN}" \
    --serial "${SERIAL}" \
    data/power.sqlite3

python ./save_generation_data.py \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${EXPORT_MPAN}" \
    --serial "${SERIAL}" \
    data/power.sqlite3

./scripts/version_db.sh
