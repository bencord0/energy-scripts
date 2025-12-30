#!/bin/bash
set -e -o pipefail

ACCOUNT_ID="A-..."
MPAN="..."
SERIAL="..."

export OCTOPUS_API_KEY="$(pass "octopus.energy/${ACCOUNT_ID}/OCTOPUS_API_KEY")"

python ./consumption.py --mpan "${MPAN}" --serial "${SERIAL}" \
    > "data/consumption-${MPAN}-${SERIAL}.json"

python ./save_consumption_data.py --account-id "${ACCOUNT_ID}" "data/consumption-${MPAN}-${SERIAL}.json" data/power.sqlite3
