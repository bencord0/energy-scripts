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

    with connection:
        for rate in rates:
            valid_from = rate['valid_from']
            valid_to = rate['valid_to']
            value = rate['value_inc_vat']

            print(f'INSERT tariff_rate for {product_code} at {valid_from}...', end='')
            try:
                connection.execute(
                    '''INSERT INTO
                        tariff_rates(
                            product_code,
                            tariff_code,
                            type,
                            valid_from,
                            valid_to,
                            value)
                        VALUES(?, ?, ?, ?, ?, ?, ?)''',
                    (product_code, tariff_code, tariff_type, valid_from, valid_to, value))
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
                product_code   TEXT, -- product code
                tariff_code    TEXT, -- per-region tariff code
                type           TEXT, -- IMPORT or EXPORT
                valid_from     TEXT, -- timestamp, use UTC date arithmetic
                valid_to       TEXT, -- timestamp, use UTC date arithmetic
                value          REAL, -- If precision is needed, use a TEXT field and integer aritmetic.
                PRIMARY KEY (product_code, tariff_code, valid_from)
            );
            COMMIT;
        ''')



if __name__ == '__main__':
    main()
