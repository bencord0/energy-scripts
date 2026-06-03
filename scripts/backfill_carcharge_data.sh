#!/bin/bash
set -ex

function save_carcharge() {
    FROM="${1}"
    TO="${2}"

    cargo run --bin save_carcharge_data -- \
        --username "${OHME_USERNAME}" \
        --password "${OHME_PASSWORD}" \
        --user-id  "${OHME_USER_ID}" \
        --from   "${FROM}" \
        --to     "${TO}" \
        data/power.sqlite3
}

save_carcharge 2026-04-30 2026-05-01

for date in $(seq 1 30); do
    save_carcharge "$(printf "2026-05-%02d" ${date})" "$(printf "2026-05-%02d" $[ ${date} + 1 ])"
done


for date in $(seq 1 3); do
    save_carcharge "$(printf "2026-06-%02d" ${date})" "$(printf "2026-06-%02d" $[ ${date} + 1 ])"
done
