# This script helps to convert python data into JSON
#
# If you accidentally `pprint(obj)`, e.g. if you `pprint(requests.get().json())`,
# then you might notice that the structure is very similar to JSON, but not exactly.
#
# ```python
# >>> obj = {
# ...  foo: 'bar',
# ...  number: 42,
# ...  beep: True,
# ... }
# >>> pprint(obj)
#
# When you actually want JSON.
# ```python
# >>> obj = ...
# >>> print(json.dumps(obj, indent=2))
#
# This scripts fixes the accident, without requiring you to redownload the data.
#
# If your muscle memory persists, consider adding
#
# ```python
# >>> pprint = lambda obj: print(json.dumps(obj, indent=2))
import json

from ast import literal_eval
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('input')
parser.add_argument('output')


def main():
    args = parser.parse_args()
    with open(args.input) as f:
        obj = literal_eval(f.read())

    with open(args.output, 'w') as f:
        f.write(json.dumps(obj, indent=2))

if __name__ == '__main__':
    main()
