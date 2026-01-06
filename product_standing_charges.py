import requests
import os
import json

from argparse import ArgumentParser
from pprint import pprint

parser = ArgumentParser()
parser.add_argument('--product-code', required=True)
parser.add_argument('--tariff-code', required=True) # includes region


def main():
    args = parser.parse_args()
    product_code = args.product_code
    tariff_code = args.tariff_code
    url = f'https://api.octopus.energy/v1/products/{product_code}/electricity-tariffs/{tariff_code}/standing-charges/'

    # This is a public API
    response = requests.get(url)
    data = response.json()

    # Hardcoded template
    data_file = f'data/products-{product_code}-{tariff_code}-standing-charges.json'
    with open(data_file, 'w') as f:
        json.dump(data, f, indent=2)
    print(f'Saved standing charges to {data_file}')


if __name__ == '__main__':
    main()
