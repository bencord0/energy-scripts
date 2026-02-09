#!/bin/bash
set -e -o pipefail

function save_agile_tariff_rates() {
    PRODUCT_CODE="${1}"
    TARIFF_CODE="${2}"

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
}

save_agile_tariff_rates AGILE-24-10-01          E-1R-AGILE-24-10-01-A
save_agile_tariff_rates AGILE-OUTGOING-19-05-13 E-1R-AGILE-OUTGOING-19-05-13-A

./scripts/version_db.sh
