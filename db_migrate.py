"""Database migrations, delegated to the Rust `migrate-db` binary.

The schema has a single source of truth: `src/migrate.rs`. Python collectors
call `migrate_db()` here instead of carrying their own `CREATE TABLE`
statements, so there is nothing to keep in sync.

Set POWER_MIGRATE_DB to a prebuilt binary to skip cargo (useful where the
toolchain isn't installed).

Example:
    from db_migrate import migrate_db

    def main():
        migrate_db('./test.sqlite3')

"""

import os
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent


def migrate_command(db_path):
    override = os.environ.get('POWER_MIGRATE_DB')
    if override:
        return [override, str(db_path)]

    # `cargo run` rebuilds when the Rust source changes, so the migration can
    # never be a stale binary.
    return [
        'cargo', 'run', '--quiet', '--bin', 'migrate-db', '--', str(db_path),
    ]


def migrate_db(db_path):
    """Bring `db_path` up to the current schema, creating it if missing.

    Raises CalledProcessError if the migration fails, rather than letting a
    collector write to an unmigrated database.
    """
    subprocess.run(migrate_command(db_path), cwd=REPO_ROOT, check=True)
