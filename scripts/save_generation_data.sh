#!/bin/bash
set -e

cargo_run save_generation_data \
    --serial "${INVERTER_SERIAL}" \
    data/power.sqlite3
