#!/bin/bash
set -e -o pipefail
set -x

function save_agile_tariff_rates() {
    PRODUCT_CODE="${1}"
    TARIFF_CODE="${2}"
    PAGE="${3}"

    python ./product_unit_rates.py \
        --product-code "${PRODUCT_CODE}" \
        --tariff-code "${TARIFF_CODE}" \
        --page "${PAGE}"

    python ./product_standing_charges.py \
        --product-code "${PRODUCT_CODE}" \
        --tariff-code "${TARIFF_CODE}"

    python ./save_tariff_data.py \
        --product-code "${PRODUCT_CODE}" \
        --tariff-code "${TARIFF_CODE}" \
        data/power.sqlite3
}

for page in $(seq 1 10); do
    save_agile_tariff_rates AGILE-OUTGOING-19-05-13 E-1R-AGILE-OUTGOING-19-05-13-A "${page}"
done
