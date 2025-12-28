import requests
import os
import json
import sys

from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--code', required=True)
parser.add_argument('--tariff', required=True)
parser.add_argument('--page', type=int)


def main():
    args = parser.parse_args()
    url = f'https://api.octopus.energy/v1/products/{args.code}/electricity-tariffs/{args.tariff}/standard-unit-rates/'

    kwargs = {}
    if page := args.page:
        kwargs['params'] = {'page': page}

    # This is a public API
    response = requests.get(url, **kwargs)
    data = response.json()
    print(json.dumps(data, indent=2))


if __name__ == '__main__':
    main()
