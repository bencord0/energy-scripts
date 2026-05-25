use chrono::{
    DateTime,
    TimeDelta,
    Timelike,
    Utc,
};
use clap::Parser;
use eyre::{
    Error,
    OptionExt,
};
use std::{
    env,
    fs,
    path,
};
use sqlx::{
    sqlite::{
        Sqlite,
        SqliteConnection,
    },
    Row,
};
use power::{
    AppState,
    FoxESSClient,
    dates::{
        dt2str,
    },
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    serial: String,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { serial, db } = Args::parse();

    let fox = FoxESSClient::new()
        .api_key(env::var("FOXESS_API_KEY")?);

    let response = fox.get_inverter_history(&serial).await?;

    let data_file = path::PathBuf::from(format!("data/generation-{serial}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&response)?)?;

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    let _ = app.acquire_pg().await?;

    migrate_db(&mut conn).await?;

    let generation = &response.generation;

    let mut start: DateTime<Utc> = generation.data[0].time.clone()
        .with_minute(0).ok_or_eyre("zero minute")?
        .with_second(0).ok_or_eyre("zero second")?
        .with_nanosecond(0).ok_or_eyre("zero nanos")?;
    let mut end: DateTime<Utc> = start + TimeDelta::minutes(30);
    let mut reference_value = generation.data[0].value;

    for data in &generation.data {
        let time = &data.time;
        let value = data.value;
        let unit = &generation.unit;

        println!("{time}: {value} {unit}");

        sqlx::query(
            "INSERT OR REPLACE INTO solar_generation_totals(
                serial,
                timestamp,
                value
            )

            VALUES (?, ?, ?);"
        )
            .bind(&serial)
            .bind(dt2str(*time))
            .bind(&value)
            .execute(&mut *conn)
            .await?;

        // We care about storing data in 30-minute intervals
        if *time < end {
            continue
        }

        let slot_value = value - reference_value;
        println!("INSERT charge for {serial} at {time} {slot_value}");

        sqlx::query(
            "INSERT OR REPLACE INTO solar_generation(
                serial,
                timestamp,
                value
            )

            VALUES (?, ?, ?);"
        )
            .bind(&serial)
            .bind(&dt2str(start))
            .bind(&slot_value)
            .execute(&mut *conn)
            .await?;

        start = end;
        end = start + TimeDelta::minutes(30);
        reference_value = value;
    }

    Ok(())
}

async fn migrate_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    sqlx::query::<Sqlite>(
        "BEGIN;

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

        CREATE INDEX IF NOT EXISTS solar_generation_timestamp ON
            solar_generation(timestamp);

        COMMIT;"
    )
        .execute(&mut *conn)
        .await?;

    Ok(())
}
