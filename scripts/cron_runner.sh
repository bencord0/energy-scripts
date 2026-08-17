#!/bin/bash
set -ex

if [[ -e config.sh ]]; then
    source config.sh
fi

cargo run --bin migrate-db
./scripts/save_agile_tariff_A.sh
./scripts/save_consumption_data.sh
./scripts/save_battery_data.sh yesterday
./scripts/save_battery_data.sh today
./scripts/save_prediction_data.sh
./scripts/save_generation_data.sh
./scripts/save_carcharge_data.sh

./scripts/version_db.sh
./scripts/rsync_upload.sh

./sqlite < ./queries/discharging_slots.sql
