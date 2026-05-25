import sqlite3
import re
import os
from datetime import datetime, timezone
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('db', help='Path to the SQLite database')
parser.add_argument('--js-file', default='src/js/db.js', help='Path to the JS file containing DB_VERSION')

def main():
    args = parser.parse_args()

    # Use the current time in UTC
    timestamp = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')

    print(f"Updating database version to: {timestamp}")

    # 1. Update the database
    connection = sqlite3.connect(args.db)
    migrate_db(connection)

    with connection:
        connection.execute(
            "INSERT OR REPLACE INTO versioning (key, value) VALUES ('version', ?)",
            (timestamp,)
        )


    # 1a. Optimise the database
    # https://www.sqlite.org/lang_analyze.html#automatically_running_analyze
    with connection:
        connection.execute("PRAGMA optimize=0x10002")
    connection.close()

    # 2. Update the JS file
    if os.path.exists(args.js_file):
        print(f"Updating {args.js_file}...")
        with open(args.js_file, 'r') as f:
            content = f.read()

        # Pattern to find const DB_VERSION = '...';
        new_content = re.sub(
            r"(const DB_VERSION = ')[^']+(';)",
            rf"\g<1>{timestamp}\g<2>",
            content
        )

        with open(args.js_file, 'w') as f:
            f.write(new_content)
        print("Done.")
    else:
        print(f"Warning: {args.js_file} not found. Skipping JS update.")

def migrate_db(connection):
    with connection:
        connection.execute('''
            CREATE TABLE IF NOT EXISTS versioning (
                key   TEXT PRIMARY KEY,
                value TEXT
            )
        ''')

if __name__ == '__main__':
    main()
