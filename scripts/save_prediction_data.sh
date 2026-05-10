#!/bin/bash
set -e

cargo run --bin save_prediction_data -- --region A ./data/power.sqlite3
