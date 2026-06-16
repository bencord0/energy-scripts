#!/bin/bash
set -e

cargo run --bin save_carcharge_data -- \
    --username "${OHME_USERNAME}" \
    --password "${OHME_PASSWORD}" \
    --user-id  "${OHME_USER_ID}" \
    --from     "$(date -d yesterday '+%Y-%m-%d')" \
    --to       "$(date -d tomorrow '+%Y-%m-%d')" \
    --login --refresh \
    data/power.sqlite3

