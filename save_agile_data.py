import sqlite3
import json

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('data')
parser.add_argument('db')


def main():
    args = parser.parse_args()

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20Electricity%20Tariff%20Standard%20Unit%20Rates
    with open(args.data) as f:
        rates = json.loads(f.read())['results']

    connection = connect_db(args.db)
    migrate_db(connection)

    with connection:
        for rate in rates:
            product = 'AGILE-24-10-01'
            tariff = 'E-1R-AGILE-24-10-01-A'
            valid_from = rate['valid_from']
            valid_to = rate['valid_to']
            value = rate['value_inc_vat']

            print(f'INSERT tarrif_rate for {product} at {valid_from}...', end='')
            try:
                connection.execute(
                    '''INSERT INTO
                        tariff_rates(
                            product,
                            tariff,
                            valid_from,
                            valid_to,
                            value)
                        VALUES(?, ?, ?, ?, ?)''',
                    (product, tariff, valid_from, valid_to, value))
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
                product    TEXT,
                tariff     TEXT,
                valid_from TEXT, -- timestamp, use UTC date arithmetic
                valid_to   TEXT, -- timestamp, use UTC date arithmetic
                value      REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                PRIMARY KEY (product, tariff, valid_from)
            );
            COMMIT;
        ''')


if __name__ == '__main__':
    main()
