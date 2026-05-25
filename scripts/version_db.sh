#!/bin/bash
set -e

python ./sync_db_version.py data/power.sqlite3 --js-file src/js/db.js
