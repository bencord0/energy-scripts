import sqlite3
import json
import os
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--account-id', required=True)
parser.add_argument('--mpan', required=True)
parser.add_argument('--serial', required=True)
parser.add_argument('db')


def main():
    args = parser.parse_args()

    data_file = f'data/consumption-{args.mpan}-{args.serial}.json'

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20consumption%20for%20an%20electricity%20meter
    with open(data_file) as f:
        results = json.loads(f.read())['results']

    connection = connect_db(args.db)
    migrate_db(connection)

    interval = last_interval(connection)

    new_data = False
    with connection:
        for data in results:
            account = args.account_id
            interval_start = data['interval_start']
            interval_end = data['interval_end']
            consumption = data['consumption']

            if interval and interval_start <= interval:
                continue
            new_data = True

            print(f'INSERT consumption for {account} at {interval_start}...', end='')
            try:
                connection.execute(
                    '''INSERT INTO
                        consumption(
                            account,
                            interval_start,
                            interval_end,
                            consumption)
                        VALUES(?, ?, ?, ?)''',
                    (account, interval_start, interval_end, consumption))
                print('OK')
            except sqlite3.IntegrityError as ie:
                print('Integrity Error')
                continue

    if not new_data:
        print('No new data')


def connect_db(uri):
    return sqlite3.connect(uri)


def migrate_db(connection):
    with connection:
        connection.executescript('''
            BEGIN;
            CREATE TABLE IF NOT EXISTS consumption (
                account        TEXT,
                interval_start TEXT, -- timestamp, use UTC date arithmetic
                interval_end   TEXT, -- timestamp, use UTC date arithmetic
                consumption    REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                generation     REAL,
                PRIMARY KEY (account, interval_start)
            );
            CREATE INDEX IF NOT EXISTS idx_consumption_start ON consumption(interval_start);
            COMMIT;
        ''')


def last_interval(connection):
    with connection:
        result = connection.execute('''
            SELECT interval_start
            FROM consumption
            ORDER BY interval_start DESC
            LIMIT 1;
        ''')
        last_interval = result.fetchone()[0]
        return last_interval


if __name__ == '__main__':
    main()
