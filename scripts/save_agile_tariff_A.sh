#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="AGILE-24-10-01"
TARIFF_ID="E-1R-AGILE-24-10-01-A"
python ./tariff_rates.py --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}" \
    > data/agile_tariff_A.json

python ./save_agile_data.py data/agile_tariff_A.json data/power.sqlite3
