import json
import os
import sqlite3
from argparse import ArgumentParser
from datetime import datetime, timedelta

from db_migrate import migrate_db

parser = ArgumentParser()
parser.add_argument('--product-code', required=True)
parser.add_argument('--tariff-code', required=True)
parser.add_argument('db')

PRODUCTS = {
    'VAR-22-11-01': 'IMPORT',
    'OUTGOING-VAR-24-10-26': 'EXPORT',
    'AGILE-24-10-01': 'IMPORT',
    'AGILE-OUTGOING-19-05-13': 'EXPORT',
}


def main():
    args = parser.parse_args()
    product_code = args.product_code
    tariff_code = args.tariff_code

    assert product_code in PRODUCTS
    tariff_type = PRODUCTS[product_code]

    standing_charges_file = f'data/products-{product_code}-{tariff_code}-standing-charges.json'
    unit_rates_file = f'data/products-{product_code}-{tariff_code}-unit-rates.json'

    # Extract standing charge
    standing_charge = 0
    with open(standing_charges_file) as f:
        standing_charge_data = json.load(f)['results']

    for rate in standing_charge_data:
        if rate.get('payment_method') in ('DIRECT_DEBIT', None):
            standing_charge = float(rate.get('value_inc_vat', 0))
            print(f'Using standing charge from {standing_charges_file}: {standing_charge} pence/day')
            break

    # Expect JSON response from
    # https://developer.octopus.energy/rest/reference/#tag/v1/operation/List%20Electricity%20Tariff%20Standard%20Unit%20Rates
    with open(unit_rates_file) as f:
        rates = json.loads(f.read())['results']

    migrate_db(args.db)
    connection = connect_db(args.db)

    # Insert/update the product mapping
    with connection:
        connection.execute(
            '''INSERT OR REPLACE INTO products(product_code, tariff_code, type, standing_charge)
               VALUES(?, ?, ?, ?)''',
            (product_code, tariff_code, tariff_type, standing_charge))

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
                            value,
                            daily_standing_charge)
                        VALUES(?, ?, ?, ?, ?, ?)''',
                    (product_code, tariff_code, valid_from, valid_to, value, standing_charge))
                print('OK')
            except sqlite3.IntegrityError as ie:
                print('IntegrityError')
                continue
    if not new_data:
        print('No new data')


def connect_db(uri):
    return sqlite3.connect(uri)


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
