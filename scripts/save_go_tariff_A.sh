#!/bin/bash
set -e -o pipefail

# Extra arguments are passed to save_tariff_rates, e.g. --since 2025-12-05.

function save_tariff_rates() {
    PRODUCT_CODE="${1}"
    TARIFF_CODE="${2}"
    shift 2

    cargo run --bin save_tariff_rates -- \
        --product-code "${PRODUCT_CODE}" \
        --tariff-code "${TARIFF_CODE}" \
        "$@" \
        data/power.sqlite3
}

save_tariff_rates GO-VAR-22-10-14 E-1R-GO-VAR-22-10-14-A "$@"

./scripts/version_db.sh
