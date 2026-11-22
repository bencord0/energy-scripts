#!/bin/bash
set -e -o pipefail

# Which tariff bills each slot (billed_tariff_rates), and two-rate tariffs'
# day/night windows. Run before any tariff script.
cargo run --bin save_agreements -- \
    --account-id "${ACCOUNT_ID}" \
    data/power.sqlite3
