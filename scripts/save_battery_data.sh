#!/bin/bash
set -e

python ./save_battery_data.py \
    --inverter-id "${INVERTER_ID}" \
    --page-size 10000 \
    --date "$(date -d "${1}" '+%Y-%m-%d')" \
    --force \
    data/power.sqlite3
