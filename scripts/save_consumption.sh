#!/bin/bash
set -e -o pipefail

ACCOUNT_ID="A-..."
MPAN="..."
SERIAL="..."

export OCTOPUS_API_KEY="$(pass "octopus.energy/${ACCOUNT_ID}/OCTOPUS_API_KEY")"

python ./consumption.py --mpan "${MPAN}" --serial "${SERIAL}"

python ./save_consumption_data.py \
    --account-id "${ACCOUNT_ID}" \
    --mpan "${MPAN}" \
    --serial "${SERIAL}" \
    data/power.sqlite3

./scripts/version_db.sh
