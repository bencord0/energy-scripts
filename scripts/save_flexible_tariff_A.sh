#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="VAR-22-11-01"
TARIFF_ID="E-1R-VAR-22-11-01-A"
python ./tariff_rates.py --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}" \
    > data/flexible_tariff_A.json

python ./save_tariff_data.py \
    --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}" \
    data/flexible_tariff_A.json data/power.sqlite3
