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

save_tariff_rates VAR-22-11-01          E-1R-VAR-22-11-01-A "$@"
save_tariff_rates OUTGOING-VAR-24-10-26 E-1R-OUTGOING-VAR-24-10-26-A "$@"

./scripts/version_db.sh
