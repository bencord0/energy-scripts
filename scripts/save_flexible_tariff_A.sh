#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="VAR-22-11-01"
TARIFF_CODE="E-1R-VAR-22-11-01-A"
python ./tariff_rates.py \
    --product-code "${PRODUCT_CODE}" \
    --tariff-code "${TARIFF_CODE}" \
    > "data/tariff_${TARIFF_CODE}.json"

python ./save_tariff_data.py \
    --product-code "${PRODUCT_CODE}" \
    --tariff-code "${TARIFF_CODE}" \
    "data/tariff_${TARIFF_CODE}.json" data/power.sqlite3
