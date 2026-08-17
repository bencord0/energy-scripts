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
use sqlx::sqlite::SqliteConnection;
use power::{
    AppState,
    FoxESSClient,
    migrate,
    dates::{
        dt2str,
    },
    times::TimeRange,
};

#[derive(Parser, Debug)]
struct Args {
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
    let Args { serial, from, to, db } = Args::parse();

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    let _ = app.acquire_pg().await?;

    let timerange: Option<TimeRange> = if let (Some(from), Some(to)) = (from, to) {
        Some(TimeRange::new_from_strs(&from, &to)?)
    } else {
        None
    };

    let fox = FoxESSClient::new()
        .api_key(env::var("FOXESS_API_KEY")?);

    let response = fox.get_inverter_history(&serial, timerange).await?;

    let data_file = path::PathBuf::from(format!("data/generation-{serial}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&response)?)?;

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

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(conn, "solar_generation_totals", &["serial", "timestamp", "value"]).await?;
    migrate::require_columns(conn, "solar_generation", &["serial", "timestamp", "value"]).await
}
