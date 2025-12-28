import requests
import os
import json

from argparse import ArgumentParser
from pprint import pprint

parser = ArgumentParser()
parser.add_argument('--code')


def main():
    args = parser.parse_args()
    url = 'https://api.octopus.energy/v1/products'
    if code := args.code:
        url += '/' + args.code

    # This is a public API
    response = requests.get(url)
    print(json.dumps(response.json(), indent=2))


if __name__ == '__main__':
    main()
