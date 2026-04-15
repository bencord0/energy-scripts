#!/usr/bin/env python
# https://agilepredict.com/api_how_to

import json
import os
import requests
import sqlite3
from argparse import ArgumentParser
from datetime import datetime, timedelta
from pathlib import Path

try:
    # Added in 3.11
    from datetime import UTC
except ImportError:
    from datetime import timezone
    UTC = timezone.utc

parser = ArgumentParser()
parser.add_argument("--region", default='A')
parser.add_argument("db")


def main():
    args = parser.parse_args()
    region = args.region

    data_file = Path(f"data/prediction-{region}.json")

    url = f"https://agilepredict.com/api/{region}/"
    response = requests.get(url)

    data = response.json()
    prices = data[0]["prices"]
    with data_file.open("w") as df:
        df.write(json.dumps(prices, indent=2))

    connection = connect_db(args.db)
    migrate_db(connection)

    with connection:
        for slot in prices:
            timestamp = str2dt(slot["date_time"])
            prediction = slot["agile_pred"]

            connection.execute(
                """INSERT OR REPLACE INTO agile_predictions
                   (region, timestamp, prediction)
                   VALUES(?, ?, ?)
                """,
                (region, dt2str(timestamp), prediction),
            )


def connect_db(uri):
    return sqlite3.connect(uri)


def migrate_db(connection):
    with connection:
        connection.executescript("""
            BEGIN;
            CREATE TABLE IF NOT EXISTS agile_predictions (
                region      TEXT,
                timestamp   TEXT, -- UTC timestamp when data was read
                prediction  REAL, -- predicted p/kWh
                PRIMARY KEY (region, timestamp)
            );
            CREATE INDEX IF NOT EXISTS agile_prediction_timestamp ON agile_predictions(timestamp);
            COMMIT;
        """)


def str2dt(dt: str) -> datetime:
    return datetime.fromisoformat(dt).astimezone(UTC)


def dt2str(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


if __name__ == "__main__":
    main()
