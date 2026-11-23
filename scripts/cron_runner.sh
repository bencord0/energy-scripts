#!/bin/bash
if [[ -e config.sh ]]; then
    source config.sh
fi

set -ex

cargo run --bin migrate-db
./scripts/save_agreements.sh

# Tariffs: uncomment the one being billed. Agile is always kept as reference.
./scripts/save_agile_tariff_A.sh
./scripts/save_intelligent_go_max_tariff_A.sh
#./scripts/save_intelligent_go_tariff_A.sh
#./scripts/save_flexible_tariff_A.sh
#./scripts/save_go_tariff_A.sh
#./scripts/save_cosy_tariff_A.sh
#./scripts/save_flux_tariff_A.sh

./scripts/save_consumption_data.sh
./scripts/save_promotion_events.sh
./scripts/save_battery_data.sh yesterday
./scripts/save_battery_data.sh today
./scripts/save_prediction_data.sh
./scripts/save_generation_data.sh
./scripts/save_carcharge_data.sh

./scripts/version_db.sh
./scripts/rsync_upload.sh

./sqlite < ./queries/discharging_slots.sql
