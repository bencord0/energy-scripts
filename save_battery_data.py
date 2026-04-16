#!/usr/bin/env python
import json
import os
import requests
import sqlite3
from argparse import ArgumentParser
from datetime import datetime, timedelta
from requests.auth import AuthBase
from pathlib import Path

try:
    # Added in 3.11
    from datetime import UTC
except ImportError:
    from datetime import timezone
    UTC = timezone.utc

parser = ArgumentParser()
parser.add_argument("--inverter-id", required=True)
parser.add_argument("--date", required=True)
parser.add_argument("--page")
parser.add_argument("--page-size", default=1000)
parser.add_argument("--force", action="store_true")
parser.add_argument("db")

GIVENERGY_API_TOKEN = os.environ["GIVENERGY_API_TOKEN"]


class BearerAuth(AuthBase):
    def __call__(self, r):
        r.headers["Authorization"] = f"Bearer {GIVENERGY_API_TOKEN}"
        return r


def main():
    args = parser.parse_args()
    inverter = args.inverter_id
    date = args.date
    force = args.force

    data_file = Path(f"data/inverter-{args.inverter_id}-{args.date}.json")

    if force or not data_file.exists():
        url = f"https://api.givenergy.cloud/v1/inverter/{inverter}/data-points/{date}"
        params = {}
        if page := args.page:
            params["page"] = page
        if page_size := args.page_size:
            params["pageSize"] = page_size

        response = requests.get(
            url,
            params=params,
            auth=BearerAuth(),
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
        )

        data = response.json()["data"]
        with data_file.open("w") as df:
            df.write(json.dumps(data, indent=2))
    else:
        with data_file.open() as df:
            data = json.loads(df.read())

    connection = connect_db(args.db)
    migrate_db(connection)

    with connection:
        start = str2dt(data[0]["time"]).replace(minute=0, second=0).astimezone(UTC)

        starttime = dt2str(start)
        end = start + timedelta(minutes=30)
        endtime = dt2str(end)

        data = iter(data)
        prev = next(data)

        for datum in data:
            datum_time = str2dt(datum["time"])
            curr_time = dt2str(datum_time)

            prev_charge_total = prev["today"]["battery"]["charge"]
            curr_charge_total = datum["today"]["battery"]["charge"]
            prev_discharge_total = prev["today"]["battery"]["discharge"]
            curr_discharge_total = datum["today"]["battery"]["discharge"]

            # Store raw values
            # These may be sampled more frequently than once a minute, but
            # are not evenly distributed.
            connection.execute(
                """INSERT OR REPLACE INTO battery_daily_totals
                   (inverter, timestamp, charge, discharge)
                   VALUES(?, ?, ?, ?)
                """,
                (inverter, curr_time, curr_charge_total, curr_discharge_total),
            )

            # We care about storing data for energy in 30-minute intervals
            # BUG: last datapoint of the day is missing
            if datum_time < end:
                continue

            # Calculate deltas, handling daily resets
            charge = curr_charge_total - prev_charge_total
            if detect_reset(prev_charge_total, curr_charge_total):
                # When reset is detected, the delta is just the current value
                # (since the counter started from 0 after the reset)
                charge = curr_charge_total

            discharge = curr_discharge_total - prev_discharge_total
            if detect_reset(prev_discharge_total, curr_discharge_total):
                discharge = curr_discharge_total

            print(f"INSERT charge for {inverter} at {start}: charge={charge:.02f} discharge={discharge:.02f}")

            connection.execute(
                """INSERT OR REPLACE INTO
                   charge(
                       inverter,
                       start,
                        end,
                       charge,
                       discharge)
                   VALUES(?, ?, ?, ?, ?)
               """,
                (inverter, starttime, endtime, charge, discharge),
             )

            prev = datum
            start = end
            end += timedelta(minutes=30)
            starttime = dt2str(start)
            endtime = dt2str(end)


def connect_db(uri):
    return sqlite3.connect(uri)


def migrate_db(connection):
    with connection:
        connection.executescript("""
            BEGIN;
            CREATE TABLE IF NOT EXISTS battery_daily_totals (
                inverter    TEXT,
                timestamp   TEXT, -- UTC timestamp when data was read
                charge      REAL, -- Raw cumulative value from inverter's "today.battery.charge"
                discharge   REAL, -- Raw cumulative value from inverter's "today.battery.discharge"
                PRIMARY KEY (inverter, timestamp)
            );
            CREATE INDEX IF NOT EXISTS battery_daily_totals_timestamp ON battery_daily_totals(timestamp);

            CREATE TABLE IF NOT EXISTS charge (
                inverter  TEXT,
                start     TEXT, -- timestamp, use UTC date arithmetic
                end       TEXT, -- timestamp, use UTC date arithmetic
                charge    REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                discharge REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                PRIMARY KEY (inverter, start)
            );
            CREATE INDEX IF NOT EXISTS charge_start ON charge(start);
            COMMIT;
        """)


def str2dt(dt: str) -> datetime:
    return datetime.fromisoformat(dt).astimezone(UTC)


def dt2str(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def detect_reset(prev_total: float, curr_total: float) -> bool:
    """
    Detect if a daily reset occurred by checking if the current total is
    significantly less than the previous total. This handles the case where
    the inverter resets its daily counters at the device's local midnight
    (which may not align with UTC midnight due to BST/GMT offset).

    Args:
        prev_total: Previous reading of the cumulative counter
        curr_total: Current reading of the cumulative counter

    Returns:
        True if a reset is detected, False otherwise
    """
    # If the current total is less than 90% of the previous, it's likely a reset
    # The 90% threshold allows for small float rounding errors but catches resets
    return curr_total < prev_total * 0.9



if __name__ == "__main__":
    main()
