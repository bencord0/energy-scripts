#!/bin/bash
set -e

cargo run --release --bin save_generation_data -- \
    --serial "${INVERTER_SERIAL}" \
    data/power.sqlite3
