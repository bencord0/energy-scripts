#!/bin/bash
set -e

cargo run --bin save_carcharge_data -- \
    --username "${OHME_USERNAME}" \
    --password "${OHME_PASSWORD}" \
    --user-id  "${OHME_USER_ID}" \
    --login --refresh \
    data/power.sqlite3

