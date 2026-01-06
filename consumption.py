import requests
import os
import json
import sys

from argparse import ArgumentParser
from datetime import datetime, timedelta, UTC

OCTOPUS_API_KEY = os.environ['OCTOPUS_API_KEY']

parser = ArgumentParser()
parser.add_argument('--mpan', required=True)
parser.add_argument('--serial', required=True)
parser.add_argument('--from')
parser.add_argument('--to')


def main():
    args = parser.parse_args()

    mpan = args.mpan
    serial = args.serial

    now = datetime.now(UTC)
    today = now.replace(hour=0, minute=0, second=0, microsecond=0)
    daybefore = today - timedelta(days=2)
    yesterday = today - timedelta(days=1)
    tomorrow = today + timedelta(days=1)

    if _from := args.__dict__['from']:
        period_from = datetime.fromisoformat(_from).replace(
            hour=0, minute=0, second=0, microsecond=0, tzinfo=UTC)
    else:
        period_from = daybefore

    if args.to:
        period_to = datetime.fromisoformat(args.to).replace(
            hour=0, minute=0, second=0, microsecond=0, tzinfo=UTC)
    else:
        period_to = tomorrow

    assert period_from < period_to, 'periods mixed up'

    print(f'period_from: {period_from}', file=sys.stderr)
    print(f'period_to:   {period_to}', file=sys.stderr)

    response = requests.get(f'https://api.octopus.energy/v1/electricity-meter-points/{mpan}/meters/{serial}/consumption',
        auth=(OCTOPUS_API_KEY, ''),
        params={
            'period_from': period_from.isoformat(),
            'period_to': period_to.isoformat(),
            'order_by': 'period',
        },
    )

    data = response.json()
    data_file = f'data/consumption-{mpan}-{serial}.json'
    with open(data_file, 'w') as f:
        json.dump(data, f, indent=2)
    print(f'Saved consumption data to {data_file}')


if __name__ == '__main__':
    main()
