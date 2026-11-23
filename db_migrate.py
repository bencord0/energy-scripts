"""Database migrations, delegated to the Rust `migrate-db` binary.

The schema has a single source of truth: `src/migrate.rs`. Python collectors
call `migrate_db()` here instead of carrying their own `CREATE TABLE` statements.

Set ENERGY_SCRIPTS_BIN_PATH to run pre-compiled binaries, otherwise this
script will use `cargo_run` to compile them from local source.

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
    bin_path = os.environ.get('ENERGY_SCRIPTS_BIN_PATH')
    if bin_path:
        return [str(Path(bin_path) / 'migrate-db'), str(db_path)]

    # `cargo run` rebuilds when the Rust source changes,
    # so the migration can use an up-to-date binary.
    cargo_flags = os.environ.get('CARGO_FLAGS')
    return [
        'cargo', 'run', '--quiet', *([cargo_flags] if cargo_flags else []),
        '--bin', 'migrate-db', '--', str(db_path),
    ]


def migrate_db(db_path):
    """Bring `db_path` up to the current schema, creating it if missing.

    Raises CalledProcessError if the migration fails, rather than letting a
    collector write to an unmigrated database.
    """
    subprocess.run(migrate_command(db_path), cwd=REPO_ROOT, check=True)
