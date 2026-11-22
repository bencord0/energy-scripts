import json
import requests
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('--product-code')
parser.add_argument('--available-at')  # ISO8601
# With --product-code: also save that tariff's standard unit rates. Time-of-use
# tariffs (Go, Flux, Cosy) publish their windows as these rows.
parser.add_argument('--tariff-code')  # includes region
parser.add_argument('--period-from')  # YYYY-MM-DD, UTC
parser.add_argument('--period-to')  # YYYY-MM-DD, UTC

URL = 'https://api.octopus.energy/v1/products'


def main():
    args = parser.parse_args()

    # This is a public API, no authentication required
    if product_code := args.product_code:
        save_product(product_code)
        if tariff_code := args.tariff_code:
            if not (args.period_from and args.period_to):
                parser.error('--tariff-code needs --period-from and --period-to')
            save_unit_rates(product_code, tariff_code, args.period_from, args.period_to)
        return

    params = {}
    if available_at := args.available_at:
        params['available_at'] = available_at
    response = requests.get(URL, params=params)
    print(json.dumps(response.json(), indent=2))


def save_product(product_code):
    response = requests.get(f'{URL}/{product_code}')
    data_file = f'data/products-{product_code}.json'
    save(data_file, response.json())
    print(f'Saved product to {data_file}')


def save_unit_rates(product_code, tariff_code, period_from, period_to):
    url = f'{URL}/{product_code}/electricity-tariffs/{tariff_code}/standard-unit-rates/'
    params = {
        'period_from': f'{period_from}T00:00:00Z',
        'period_to': f'{period_to}T00:00:00Z',
        'page_size': 1500,
    }
    data = requests.get(url, params=params).json()
    results = data['results']
    while next_url := data.get('next'):
        data = requests.get(next_url).json()
        results += data['results']

    data_file = f'data/products-{product_code}-{tariff_code}-unit-rates-{period_from}-{period_to}.json'
    save(data_file, {'count': len(results), 'next': None, 'previous': None, 'results': results})
    print(f'Saved {len(results)} unit rates to {data_file}')


def save(data_file, data):
    with open(data_file, 'w') as f:
        json.dump(data, f, indent=2)


if __name__ == '__main__':
    main()
