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

# Intelligent Flux has no public export product, so only its import is fetched.
save_tariff_rates FLUX-IMPORT-23-02-14         E-1R-FLUX-IMPORT-23-02-14-A "$@"
save_tariff_rates INTELLI-FLUX-IMPORT-23-07-14 E-1R-INTELLI-FLUX-IMPORT-23-07-14-A "$@"
save_tariff_rates FLUX-EXPORT-23-02-14         E-1R-FLUX-EXPORT-23-02-14-A "$@"

./scripts/version_db.sh
