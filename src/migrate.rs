use eyre::{Error, eyre};
use sqlx::sqlite::{Sqlite, SqliteConnection};

// Version 1
const V1_SCHEMA: &str = "BEGIN;

    CREATE TABLE IF NOT EXISTS carcharge (
        id        TEXT,
        timestamp TEXT, -- use UTC date arithmetic
        charge    REAL, -- kWh
        PRIMARY KEY (id, timestamp)
    );
    CREATE INDEX IF NOT EXISTS carcharge_timestamp ON carcharge(timestamp);

    CREATE TABLE IF NOT EXISTS consumption (
        account        TEXT,
        interval_start TEXT, -- timestamp, use UTC date arithmetic
        interval_end   TEXT, -- timestamp, use UTC date arithmetic
        consumption    REAL, -- If precision is needed, use a TEXT field and integer arithmetic
        generation     REAL,
        PRIMARY KEY (account, interval_start)
    );
    CREATE INDEX IF NOT EXISTS idx_consumption_start ON consumption(interval_start);

    CREATE TABLE IF NOT EXISTS solar_generation_totals (
        serial    TEXT,
        timestamp TEXT, -- timestamp, use UTC date arithmetic
        value     REAL, -- Raw cumulative value
        PRIMARY KEY (serial, timestamp)
    );

    CREATE TABLE IF NOT EXISTS solar_generation (
        serial    TEXT,
        timestamp TEXT, -- timestamp, use UTC date arithmetic
        value     REAL, -- actual kWh
        PRIMARY KEY (serial, timestamp)
    );
    CREATE INDEX IF NOT EXISTS solar_generation_timestamp ON solar_generation(timestamp);

    CREATE TABLE IF NOT EXISTS agile_predictions (
        region     TEXT,
        timestamp  TEXT, -- timestamp, use UTC date arithmetic
        prediction REAL, -- predicted p/kWh
        PRIMARY KEY (region, timestamp)
    );
    CREATE INDEX IF NOT EXISTS agile_prediction_timestamp ON agile_predictions(timestamp);

    CREATE TABLE IF NOT EXISTS products (
        product_code    TEXT NOT NULL,
        tariff_code     TEXT NOT NULL,
        type            TEXT NOT NULL, -- IMPORT or EXPORT
        standing_charge REAL,
        PRIMARY KEY (product_code, tariff_code)
    );

    CREATE TABLE IF NOT EXISTS tariff_rates (
        product_code TEXT NOT NULL,
        tariff_code  TEXT NOT NULL, -- per-region tariff code
        valid_from   TEXT NOT NULL, -- timestamp, use UTC date arithmetic
        valid_to     TEXT,          -- timestamp, use UTC date arithmetic
        value        REAL,          -- If precision is needed, use a TEXT field and integer arithmetic
        daily_standing_charge REAL,
        PRIMARY KEY (product_code, tariff_code, valid_from)
    );

    COMMIT;";

pub async fn migrate(conn: &mut SqliteConnection) -> Result<(), Error> {
    let mut version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;

    if version < 1 {
        sqlx::query::<Sqlite>(V1_SCHEMA).execute(&mut *conn).await?;
        version = 1;
    }

    if version < 2 {
        add_column_if_missing(conn, "agile_predictions", "export_prediction", "REAL").await?;
        version = 2;
    }

    if version < 3 {
        rename_column_if_present(conn, "agile_predictions", "prediction", "import_prediction")
            .await?;
        version = 3;
    }

    // Next migration step goes here:
    // if version < 4 { ...; version = 4; }

    // PRAGMA can't take bind params; `version` is an internal constant.
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(&mut *conn)
        .await?;

    Ok(())
}

/// Declared SQLite column type, as it appears in `CREATE TABLE`. Compared
/// case-insensitively against `pragma_table_info.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlType {
    Text,
    Real,
    Integer,
}

impl SqlType {
    fn matches(self, declared: &str) -> bool {
        let declared = declared.trim();
        match self {
            SqlType::Text => declared.eq_ignore_ascii_case("TEXT"),
            SqlType::Real => declared.eq_ignore_ascii_case("REAL"),
            SqlType::Integer => declared.eq_ignore_ascii_case("INTEGER"),
        }
    }
}

/// Assert `table` exists and carries every `(name, type)` in `columns`. Used by each
/// binary's `check_db()` to fail fast (non-zero exit) against a database that hasn't
/// been migrated or has drifted to a mis-typed schema.
pub async fn require_columns(
    conn: &mut SqliteConnection,
    table: &str,
    columns: &[(&str, SqlType)],
) -> Result<(), Error> {
    let present = table_columns(conn, table).await?;
    if present.is_empty() {
        return Err(eyre!("missing table `{table}`; run the migrate-db binary"));
    }

    for (name, expected) in columns {
        match present.iter().find(|(n, _)| n == name) {
            None => {
                return Err(eyre!(
                    "table `{table}` missing column `{name}`; run the migrate-db binary"
                ));
            }
            Some((_, declared)) if !expected.matches(declared) => {
                return Err(eyre!(
                    "table `{table}` column `{name}` has type `{declared}`, \
                     expected {expected:?}; run the migrate-db binary"
                ));
            }
            Some(_) => {}
        }
    }

    Ok(())
}

async fn rename_column_if_present(
    conn: &mut SqliteConnection,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), Error> {
    let cols = table_columns(conn, table).await?;
    if cols.iter().any(|(n, _)| n == from) && !cols.iter().any(|(n, _)| n == to) {
        sqlx::query(&format!(
            "ALTER TABLE {table} RENAME COLUMN {from} TO {to}"
        ))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn add_column_if_missing(
    conn: &mut SqliteConnection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<(), Error> {
    if !table_columns(conn, table).await?.iter().any(|(n, _)| n == column) {
        sqlx::query(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// `(name, type)` of each column in `table`, empty if it doesn't exist. `table` is
/// string-interpolated (it's an internal constant, never user input), which sidesteps
/// bound-parameter quirks with the table-valued `pragma_table_info` function.
async fn table_columns(
    conn: &mut SqliteConnection,
    table: &str,
) -> Result<Vec<(String, String)>, Error> {
    let cols = sqlx::query_as::<_, (String, String)>(&format!(
        "SELECT name, type FROM pragma_table_info('{table}')"
    ))
    .fetch_all(&mut *conn)
    .await?;
    Ok(cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;

    async fn memory_db() -> SqliteConnection {
        SqliteConnection::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn migrate_is_idempotent() -> Result<(), Error> {
        let mut conn = memory_db().await;

        migrate(&mut conn).await?;
        // Regression: the old per-binary ALTER errored on the second run.
        migrate(&mut conn).await?;

        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&mut conn)
            .await?;
        assert_eq!(version, 3);
        Ok(())
    }

    #[tokio::test]
    async fn migrate_upgrades_legacy_agile_predictions() -> Result<(), Error> {
        let mut conn = memory_db().await;

        // Legacy shape: table present without export_prediction, user_version still 0.
        sqlx::query(
            "CREATE TABLE agile_predictions (
                region TEXT, timestamp TEXT, prediction REAL,
                PRIMARY KEY (region, timestamp))",
        )
        .execute(&mut conn)
        .await?;

        migrate(&mut conn).await?;

        let cols = table_columns(&mut conn, "agile_predictions").await?;
        assert!(cols.iter().any(|(n, _)| n == "export_prediction"));
        Ok(())
    }

    #[tokio::test]
    async fn migrate_renames_prediction_to_import_prediction() -> Result<(), Error> {
        let mut conn = memory_db().await;

        sqlx::query(
            "CREATE TABLE agile_predictions (
                region TEXT, timestamp TEXT, prediction REAL,
                PRIMARY KEY (region, timestamp))",
        )
        .execute(&mut conn)
        .await?;

        migrate(&mut conn).await?;

        let cols = table_columns(&mut conn, "agile_predictions").await?;
        assert!(cols.iter().any(|(n, _)| n == "import_prediction"));
        assert!(!cols.iter().any(|(n, _)| n == "prediction"));
        Ok(())
    }

    #[tokio::test]
    async fn require_columns_detects_gaps() -> Result<(), Error> {
        let mut conn = memory_db().await;

        // Missing table before migration.
        assert!(
            require_columns(&mut conn, "carcharge", &[("id", SqlType::Text)])
                .await
                .is_err()
        );

        migrate(&mut conn).await?;

        require_columns(
            &mut conn,
            "carcharge",
            &[
                ("id", SqlType::Text),
                ("timestamp", SqlType::Text),
                ("charge", SqlType::Real),
            ],
        )
        .await?;

        // Missing column.
        assert!(
            require_columns(&mut conn, "carcharge", &[("nope", SqlType::Text)])
                .await
                .is_err()
        );

        // Present column, wrong declared type.
        assert!(
            require_columns(&mut conn, "carcharge", &[("charge", SqlType::Text)])
                .await
                .is_err()
        );
        Ok(())
    }
}
