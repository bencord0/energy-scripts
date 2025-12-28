import sqlite3
import json
from datetime import datetime, timedelta

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--code', required=True)
parser.add_argument('--tariff', required=True)
parser.add_argument('data')
parser.add_argument('db')


def main():
    args = parser.parse_args()
    product = args.code
    tariff = args.tariff

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20Electricity%20Tariff%20Standard%20Unit%20Rates
    with open(args.data) as f:
        rates = json.loads(f.read())['results']

    rates = as_half_hourly_rates(rates)

    connection = connect_db(args.db)
    migrate_db(connection)

    with connection:
        for rate in rates:
            valid_from = rate['valid_from']
            valid_to = rate['valid_to']
            value = rate['value_inc_vat']

            print(f'INSERT tariff_rate for {product} at {valid_from}...', end='')
            try:
                connection.execute(
                    '''INSERT INTO
                        tariff_rates(
                            valid_from,
                            valid_to,
                            value)
                        VALUES(?, ?, ?)''',
                    (valid_from, valid_to, value))
                print('OK')
            except sqlite3.IntegrityError as ie:
                print('IntegrityError')
                continue


def connect_db(uri):
    return sqlite3.connect(uri)


def migrate_db(connection):
    with connection:
        connection.executescript('''
            BEGIN;
            CREATE TABLE IF NOT EXISTS tariff_rates (
                valid_from     TEXT, -- timestamp, use UTC date arithmetic
                valid_to       TEXT, -- timestamp, use UTC date arithmetic
                value          REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                PRIMARY KEY (valid_from)
            );
            COMMIT;
        ''')


def as_half_hourly_rates(original):
    results = []
    horizon = datetime.fromisoformat('2025-12-27T00:00:00Z')

    for rate in original:
        period_start = datetime.fromisoformat(rate['valid_from'])

        if rate['valid_to'] is None:
            continue
        period_until = datetime.fromisoformat(rate['valid_to'])

        if (period_until - period_start) <= timedelta(minutes=30):
            results.append(rate)
            continue

        period_end = period_start + timedelta(minutes=30)
        value = rate['value_inc_vat']

        while period_end < period_until:
            rate = {
                'valid_from': period_start.isoformat().replace('+00:00', 'Z'),
                'valid_to': period_end.isoformat().replace('+00:00', 'Z'),
                'value_inc_vat': value,
            }
            results.append(rate)

            period_start = period_end
            period_end = period_end + timedelta(minutes=30)

            if period_start >= horizon:
                break

    return results


if __name__ == '__main__':
    main()
