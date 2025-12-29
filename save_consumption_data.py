import sqlite3
import json

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--account-id', required=True)
parser.add_argument('data')
parser.add_argument('db')


def main():
    args = parser.parse_args()

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20consumption%20for%20an%20electricity%20meter
    with open(args.data) as f:
        results = json.loads(f.read())['results']

    connection = connect_db(args.db)
    migrate_db(connection)

    with connection:
        for data in results:
            account = args.account_id
            interval_start = data['interval_start']
            interval_end = data['interval_end']
            consumption = data['consumption']

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
                PRIMARY KEY (account, interval_start)
            );
            CREATE INDEX IF NOT EXISTS idx_consumption_start ON consumption(interval_start);
            COMMIT;
        ''')


if __name__ == '__main__':
    main()
