#!/bin/bash
set -e

python ./save_battery_data.py \
    --octopus-account "${ACCOUNT_ID}" \
    --inverter-ids "${INVERTER_IDS}" \
    --page-size 10000 \
    --date "$(date -d "${1}" '+%Y-%m-%d')" \
    --force \
    data/power.sqlite3
