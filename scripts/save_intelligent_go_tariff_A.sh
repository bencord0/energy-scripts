#!/bin/bash
set -e -o pipefail

# Extra arguments are passed to save_tariff_rates, e.g. --since 2025-12-05.

function save_tariff_rates() {
    PRODUCT_CODE="${1}"
    TARIFF_CODE="${2}"
    shift 2

    cargo_run save_tariff_rates \
        --product-code "${PRODUCT_CODE}" \
        --tariff-code "${TARIFF_CODE}" \
        "$@" \
        data/power.sqlite3
}

# Dispatches re-price slots to off-peak, so must land before the rates. The
# day/night windows come from the account's agreement (save_agreements.sh).
cargo_run save_dispatches \
    --account-id "${ACCOUNT_ID}" \
    data/power.sqlite3

save_tariff_rates IOG-SMB-VAR-26-11-04 E-1R-IOG-SMB-VAR-26-11-04-A "$@"

./scripts/version_db.sh
