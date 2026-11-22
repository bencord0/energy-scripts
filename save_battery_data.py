#!/usr/bin/env python
import json
import os
import requests
import sqlite3
import sys
from argparse import ArgumentParser
from datetime import datetime, timedelta
from pathlib import Path
from requests.auth import AuthBase

from db_migrate import migrate_db
from grid_slots import save_consumption_estimates

try:
    # Added in 3.11
    from datetime import UTC
except ImportError:
    from datetime import timezone
    UTC = timezone.utc

parser = ArgumentParser()
parser.add_argument("--octopus-account")
parser.add_argument("--inverter-ids", required=True)
parser.add_argument("--date", required=True)
parser.add_argument("--page")
parser.add_argument("--page-size", default=10000)
parser.add_argument("--force", action="store_true")
parser.add_argument("db")

GIVENERGY_API_TOKEN = os.environ["GIVENERGY_API_TOKEN"]


class BearerAuth(AuthBase):
    def __call__(self, r):
        r.headers["Authorization"] = f"Bearer {GIVENERGY_API_TOKEN}"
        return r


def main():
    args = parser.parse_args()
    octopus_account = args.octopus_account
    inverters = args.inverter_ids.split(',')
    date = args.date
    force = args.force
    page = args.page
    page_size = args.page_size

    starts = []
    for inverter in inverters:
        data = fetch_inverter_data(date, inverter, force, page, page_size)
        starts += save_inverter_data(args.db, inverter, data)

    # Combined across inverters, so one glitching counter can't skew a slot.
    if octopus_account and starts:
        connection = connect_db(args.db)
        with connection:
            save_consumption_estimates(connection, octopus_account, starts)


def fetch_inverter_data(date, inverter, force, page, page_size):
    data_file = Path(f"data/inverter-{inverter}-{date}.json")

    if force or not data_file.exists():
        url = f"https://api.givenergy.cloud/v1/inverter/{inverter}/data-points/{date}"
        params = {}
        if page:
            params["page"] = page
        if page_size:
            params["pageSize"] = page_size

        try:
            response = requests.get(
                url,
                params=params,
                auth=BearerAuth(),
                headers={
                    "Content-Type": "application/json",
                    "Accept": "application/json",
                },
            )
        except requests.exceptions.SSLError as e:
            print("SSL error on connection: api.givenergy.cloud")
            sys.exit(0)

        data = response.json()["data"]
        with data_file.open("w") as df:
            df.write(json.dumps(data, indent=2))
    else:
        with data_file.open() as df:
            data = json.loads(df.read())

    return data

def save_inverter_data(db, inverter, data):
    """Save raw totals and per-slot battery deltas.

    Returns the slot starts whose grid estimate may have changed, including
    the previous day's 23:30 slot, which this file's first sample closes.
    """
    starts = []
    if len(data) < 1:
        return starts

    migrate_db(db)
    connection = connect_db(db)

    with connection:
        start = str2dt(data[0]["time"]).replace(minute=0, second=0).astimezone(UTC)

        starttime = dt2str(start)
        end = start + timedelta(minutes=30)
        endtime = dt2str(end)

        starts.append(dt2str(start - timedelta(minutes=30)))

        data = iter(data)
        prev = next(data)
        save_raw_totals(connection, inverter, prev)

        for datum in data:
            datum_time = str2dt(datum["time"])
            save_raw_totals(connection, inverter, datum)

            # Need to use daily numbers, as 'total' is corrupt
            prev_charge_total    = prev ["today"]["battery"]["charge"]
            curr_charge_total    = datum["today"]["battery"]["charge"]
            prev_discharge_total = prev ["today"]["battery"]["discharge"]
            curr_discharge_total = datum["today"]["battery"]["discharge"]

            # We care about storing data for energy in 30-minute intervals
            # BUG: the day's last slot isn't saved to `charge`
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

            starts.append(starttime)

            prev = datum
            start = end
            end += timedelta(minutes=30)
            starttime = dt2str(start)
            endtime = dt2str(end)

    return starts


def connect_db(uri):
    return sqlite3.connect(uri)


def save_raw_totals(connection, inverter, datum):
    """Store one sample's raw counters.

    Samples may be more frequent than once a minute, but are not evenly
    distributed. The grid totals don't reset daily, so slot estimates are
    derived from them (see `grid_slots`).
    """
    timestamp = dt2str(str2dt(datum["time"]))
    connection.execute(
        """INSERT OR REPLACE INTO battery_daily_totals
           (inverter, timestamp, charge, discharge)
           VALUES(?, ?, ?, ?)
        """,
        (
            inverter,
            timestamp,
            datum["today"]["battery"]["charge"],
            datum["today"]["battery"]["discharge"],
        ),
    )
    connection.execute(
        """INSERT OR REPLACE INTO battery_grid_totals
           (inverter, timestamp, import, export)
           VALUES(?, ?, ?, ?)
        """,
        (
            inverter,
            timestamp,
            datum["total"]["grid"]["import"],
            datum["total"]["grid"]["export"],
        ),
    )


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
