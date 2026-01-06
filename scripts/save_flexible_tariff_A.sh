#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="VAR-22-11-01"
TARIFF_CODE="E-1R-VAR-22-11-01-A"
python ./product_unit_rates.py \
    --product-code "${PRODUCT_CODE}" \
    --tariff-code "${TARIFF_CODE}"

python ./product_standing_charges.py \
    --product-code "${PRODUCT_CODE}" \
    --tariff-code "${TARIFF_CODE}"

python ./save_tariff_data.py \
    --product-code "${PRODUCT_CODE}" \
    --tariff-code "${TARIFF_CODE}" \
    data/power.sqlite3
