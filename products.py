import requests
import os
import json

from argparse import ArgumentParser
from pprint import pprint

parser = ArgumentParser()
parser.add_argument('--product-code')
parser.add_argument('--available-at')  # ISO8601


def main():
    args = parser.parse_args()
    url = 'https://api.octopus.energy/v1/products'

    # This is a public API, no authentication required
    if product_code := args.product_code:
        url += '/' + product_code
        response = requests.get(url)
        data = response.json()

        data_file = f'data/products-{product_code}.json'
        with open(data_file, 'w') as f:
            json.dump(data, f, indent=2)
        print(f'Saved product to {data_file}')
        return

    params = {}
    if available_at := args.available_at:
        params['available_at'] = available_at
    response = requests.get(url, params=params)
    print(json.dumps(response.json(), indent=2))


if __name__ == '__main__':
    main()
