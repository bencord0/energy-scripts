use clap::Parser;
use chrono::{
    DateTime,
    Utc,
};
use eyre::Error;
use sqlx::{
    Row,
    sqlite::{
        Sqlite,
        SqliteConnection,
    },
};
use std::{
    fs,
    path::PathBuf,
};
use power::{
    AppState,
    AgilePredictClient,
    dates::{
        dt2str,
        str2dt,
    },
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value="A")]
    region: String,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { region, db } = Args::parse();

    let predictor = AgilePredictClient::new();
    let predictions = predictor.get_prediction(&region).await?;

    let data_file = PathBuf::from(format!("data/prediction-{region}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&predictions)?)?;

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    let _ = app.acquire_pg().await?;

    migrate_db(&mut conn).await?;
    let interval = last_interval(
        &mut conn,
        "AGILE-24-10-01",
        "E-1R-AGILE-24-10-01-A",
    ).await?;

    for slot in &predictions[0].prices {
        let timestamp = str2dt(&slot.date_time)?;
        let prediction = &slot.agile_pred;

        sqlx::query(
            "INSERT OR REPLACE INTO agile_predictions(
                region,
                timestamp,
                prediction
            )

            VALUES (?, ?, ?);"
        )
            .bind(&region)
            .bind(dt2str(timestamp))
            .bind(&prediction)
            .execute(&mut *conn)
            .await?;


        sqlx::query(
            "DELETE FROM agile_predictions
            WHERE region = ?
              AND timestamp < ?"
        )
            .bind(&region)
            .bind(&dt2str(interval))
            .execute(&mut *conn)
            .await?;
    }

    Ok(())
}

async fn last_interval(
    conn: &mut SqliteConnection,
    product_code: &str,
    tariff_code: &str,
) -> Result<DateTime<Utc>, Error>
{
    let rows = sqlx::query::<Sqlite>(
        "SELECT
            valid_from
        FROM tariff_rates

        WHERE product_code = ?
          AND tariff_code  = ?

        ORDER BY valid_from DESC
        LIMIT 1"
    )
        .bind(&product_code)
        .bind(&tariff_code)
        .fetch_one(&mut *conn)
        .await?;

    let interval = rows.get(0);
    Ok(str2dt(interval)?)
}

async fn migrate_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    sqlx::query::<Sqlite>(
        "BEGIN;

        CREATE TABLE IF NOT EXISTS agile_predictions (
            region        TEXT,
            timestamp     TEXT, -- timestamp, use UTC date aritmetic
            prediction    REAL, -- predicted p/kWh
            PRIMARY KEY (region, timestamp)

        );

        CREATE INDEX IF NOT EXISTS agile_prediction_timestamp
            ON agile_predictions(timestamp);

        COMMIT;"
    )
        .execute(&mut *conn)
        .await?;

    Ok(())
}
