import requests
import os
import json
import sys

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--product-code', required=True)
parser.add_argument('--tariff-code', required=True)
parser.add_argument('--page', type=int)


def main():
    args = parser.parse_args()
    product_code = args.product_code
    tariff_code = args.tariff_code
    url = f'https://api.octopus.energy/v1/products/{product_code}/electricity-tariffs/{tariff_code}/standard-unit-rates/'

    kwargs = {}
    if page := args.page:
        kwargs['params'] = {'page': page}

    # This is a public API
    response = requests.get(url, **kwargs)
    try:
        data = response.json()
    except Exception as e:
        print(e)
        breakpoint()
    print(json.dumps(data, indent=2))


if __name__ == '__main__':
    main()
