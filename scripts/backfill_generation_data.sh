#!/bin/bash
set -ex

function save_generation() {
    FROM="${1}"
    TO="${2}"

    cargo run --bin save_generation_data -- \
        --serial "${INVERTER_SERIAL}" \
        --from   "${FROM}" \
        --to     "${TO}" \
        data/power.sqlite3
}

for DATE in $(seq 8 23); do
    save_generation "$(printf "2026-05-%02d" ${DATE})" "$(printf "2026-05-%02d" $[ ${DATE} + 1 ])"
done
