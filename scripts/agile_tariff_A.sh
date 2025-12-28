#!/bin/bash
set -e -o pipefail

PRODUCT_CODE="AGILE-24-10-01"
TARIFF_ID="E-1R-AGILE-24-10-01-A"
python ./tariff_rates.py --code "${PRODUCT_CODE}" --tariff "${TARIFF_ID}"
