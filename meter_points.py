import requests
import os
import json

from argparse import ArgumentParser

OCTOPUS_API_KEY = os.environ['OCTOPUS_API_KEY']

parser = ArgumentParser()
parser.add_argument('--mpan', required=True)


def main():
    args = parser.parse_args()

    mpan = args.mpan
    response = requests.get(f'https://api.octopus.energy/v1/electricity-meter-points/{mpan}/',
        auth=(OCTOPUS_API_KEY, ''),
    )

    print(json.dumps(response.json(), indent=2))


if __name__ == '__main__':
    main()
