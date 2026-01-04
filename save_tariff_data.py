import sqlite3
import json
from datetime import datetime, timedelta

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--product-code', required=True)
parser.add_argument('--tariff-code', required=True)
parser.add_argument('data')
parser.add_argument('db')

PRODUCTS = {
    # Fixed Rate Tariffs, ignore these for now
    #'VAR-22-11-01': 'IMPORT',
    #'OUTGOING-VAR-24-10-26': 'EXPORT',

    'AGILE-24-10-01': 'IMPORT',
    'AGILE-OUTGOING-19-05-13': 'EXPORT',
}

def main():
    args = parser.parse_args()
    product_code = args.product_code
    tariff_code = args.tariff_code

    assert product_code in PRODUCTS
    tariff_type = PRODUCTS[product_code]

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20Electricity%20Tariff%20Standard%20Unit%20Rates
    with open(args.data) as f:
        rates = json.loads(f.read())['results']

    connection = connect_db(args.db)
    migrate_db(connection)

    # Insert/update the product mapping
    with connection:
        connection.execute(
            '''INSERT OR REPLACE INTO products(product_code, tariff_code, type)
               VALUES(?, ?, ?)''',
            (product_code, tariff_code, tariff_type))

    interval = last_interval(product_code, tariff_code, connection)

    new_data = False
    with connection:
        for rate in rates:
            valid_from = rate['valid_from']
            valid_to = rate['valid_to']
            value = rate['value_inc_vat']

            if interval and valid_from <= interval:
                continue
            new_data = True

            print(f'INSERT tariff_rate for {product_code} at {valid_from}...', end='')
            try:
                connection.execute(
                    '''INSERT INTO
                        tariff_rates(
                            product_code,
                            tariff_code,
                            valid_from,
                            valid_to,
                            value)
                        VALUES(?, ?, ?, ?, ?)''',
                    (product_code, tariff_code, valid_from, valid_to, value))
                print('OK')
            except sqlite3.IntegrityError as ie:
                print('IntegrityError')
                continue
    if not new_data:
        print('No new data')


def connect_db(uri):
    return sqlite3.connect(uri)


def migrate_db(connection):
    with connection:
        connection.executescript('''
            BEGIN;
            CREATE TABLE IF NOT EXISTS products (
                product_code TEXT NOT NULL,
                tariff_code  TEXT NOT NULL,
                type         TEXT NOT NULL, -- IMPORT or EXPORT
                PRIMARY KEY (product_code, tariff_code)
            );
            CREATE TABLE IF NOT EXISTS tariff_rates (
                product_code TEXT NOT NULL, -- product code
                tariff_code  TEXT NOT NULL, -- per-region tariff code
                valid_from   TEXT NOT NULL, -- timestamp, use UTC date arithmetic
                valid_to     TEXT,          -- timestamp, use UTC date arithmetic
                value        REAL,          -- If precision is needed, use a TEXT field and integer aritmetic.
                PRIMARY KEY (product_code, tariff_code, valid_from)
            );
            COMMIT;
        ''')


def last_interval(product_code, tariff_code, connection):
    with connection:
        result = connection.execute('''
            SELECT valid_from
            FROM tariff_rates
            WHERE product_code = :product_code
                AND tariff_code = :tariff_code
            ORDER BY valid_from DESC
            LIMIT 1;
        ''', {
            'product_code': product_code,
            'tariff_code': tariff_code,
        })
        last_interval = result.fetchone()
        if last_interval:
            return last_interval[0]

if __name__ == '__main__':
    main()
