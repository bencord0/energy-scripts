import requests
import os
import json

from argparse import ArgumentParser

OCTOPUS_API_KEY = os.environ['OCTOPUS_API_KEY']

parser = ArgumentParser()
parser.add_argument('--account-id', required=True)


def main():
    args = parser.parse_args()

    account_id = args.account_id
    response = requests.get(f'https://api.octopus.energy/v1/accounts/{account_id}/',
        auth=(OCTOPUS_API_KEY, ''),
    )

    try:
        data = response.json()
    except Exception:
        breakpoint()
        raise

    print(json.dumps(data, indent=2))


if __name__ == '__main__':
    main()
