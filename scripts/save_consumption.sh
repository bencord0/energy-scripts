#!/bin/bash
set -e -o pipefail

ACCOUNT_ID="A-..."
MPAN="............."
SERIAL=".........."

export OCTOPUS_API_KEY="$(pass "octopus.energy/${ACCOUNT_ID}/OCTOPUS_API_KEY")"

for FROM in $(seq 1 31); do
    TO=$[$FROM + 1]
python ./consumption.py --mpan "${MPAN}" --serial "${SERIAL}" \
    --from "2025-12-$(printf %02d $FROM)T00:00:00Z" \
    --to   "2025-12-$(printf %02d $TO)T00:00:00Z" \
    > "data/consumption-${MPAN}-${SERIAL}.json"

python ./save_consumption_data.py --account-id "${ACCOUNT_ID}" "data/consumption-${MPAN}-${SERIAL}.json" data/power.sqlite3
done
