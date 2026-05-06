use chrono::{
    DateTime,
    Utc,
};
use clap::Parser;
use eyre::{Error, Context};
use std::{
    env,
    fs,
    path::PathBuf,
    time::Duration,
};
use sqlx::{
    sqlite::{
        Sqlite,
        SqliteConnection,
        SqlitePool,
    },
    postgres::{
        Postgres,
        PgPool,
    },
    pool::{PoolConnection, PoolOptions},
    Row,
};
use power::{
    AppState,
    OctopusClient,
    dates::{
        dt2str,
        str2dt,
    },
};

#[derive(Parser,Debug)]
struct Args {
    #[arg(long)]
    account_id: String,
    #[arg(long)]
    mpan: String,
    #[arg(long)]
    serial: String,

    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { account_id, mpan, serial, db, from, to } = Args::parse();

    let octopus = OctopusClient::new()
        .api_key(env::var("OCTOPUS_API_KEY")?);

    // XXX: On failure, attempt a read from cache
    let from = from.map(|f| str2dt(&f).expect("not datestamp"));
    let to = to.map(|t| str2dt(&t).expect("not datestamp"));
    let response = octopus.get_consumption(&mpan, &serial, from, to).await?;

    // If request is successful, cache the response to a file
    let data_file = PathBuf::from(format!("data/consumption-{mpan}-{serial}.json"));
    fs::write(&data_file, serde_json::to_string(&response)?.as_bytes())?;

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    let _ = app.acquire_pg().await?;

    migrate_db(&mut conn).await?;
    let interval = last_interval(&mut conn).await;

    for data in response.results {
        let interval_start = str2dt(&data.interval_start)?;
        let interval_end = str2dt(&data.interval_end)?;
        let consumption = data.consumption.clone();

        if let Ok(interval) = interval {
            if interval_start <= interval {
                continue
            }
        }

        print!("INSERT consumption for {account_id} at {interval_start}...");
        sqlx::query::<Sqlite>(
            "INSERT INTO consumption(
                account,
                interval_start,
                interval_end,
                consumption)
            VALUES($1, $2, $3, $4)"
        )
            .bind(&account_id)
            .bind(dt2str(interval_start))
            .bind(dt2str(interval_end))
            .bind(consumption)
            .execute(&mut *conn)
            .await?;

    }

    println!("Saved consumption data to {data_file:?}");
    Ok(())
}

async fn last_interval(conn: &mut SqliteConnection) -> Result<DateTime<Utc>, Error> {
    let rows = sqlx::query::<Sqlite>(
        "SELECT
            interval_start
        FROM consumption
        WHERE consumption IS NOT NULL
        ORDER BY interval_start DESC
        LIMIT 1"
    )
        .fetch_one(&mut *conn)
        .await?;

    let interval_start = rows.get("interval_start");
    println!("last_interval: {interval_start}");
    let last_interval = str2dt(interval_start)?;
    Ok(last_interval)
}

async fn migrate_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    sqlx::query::<Sqlite>(
        "BEGIN;

        CREATE TABLE IF NOT EXISTS consumption (
            account        TEXT,
            interval_start TEXT, -- timestamp, use UTC date aritmetic
            interval_end   TEXT, -- timestamp, use UTC date aritmetic
            consumption    REAL, -- If precision is needed, use a TEXT field and integer arithmetic
            generation     REAL,
            PRIMARY KEY (account, interval_start)

        );

        CREATE INDEX IF NOT EXISTS idx_consumption_start
            ON consumption(interval_start);

        COMMIT;"
    )
        .execute(&mut *conn)
        .await?;

    Ok(())
}
