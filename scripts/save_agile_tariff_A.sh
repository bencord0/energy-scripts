#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="AGILE-24-10-01"
TARIFF_ID="E-1R-AGILE-24-10-01-A"

for PAGE in $(seq 1 3); do
python ./tariff_rates.py --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}" \
    --page "${PAGE}" \
    > data/agile_tariff_A.json

python ./save_tariff_data.py \
    --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}" \
    data/agile_tariff_A.json data/power.sqlite3
done
