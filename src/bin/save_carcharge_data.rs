use chrono::{DateTime, Utc};
use clap::Parser;
use eyre::{Error, OptionExt};
use std::{
    path,
};
use sqlx::sqlite::{
    Sqlite,
    SqliteConnection,
};
use power::{
    AppState,
    OhmeClient,
    dates::dt2str,
    times::TimeRange,
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    username: String,
    #[arg(long)]
    password: String,
    #[arg(long)]
    user_id: String,

    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,

    #[arg(long)]
    login: bool,
    #[arg(long)]
    refresh: bool,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { username, password, user_id, from, to, login, refresh, db } = Args::parse();

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    let _ = app.acquire_pg().await?;

    migrate_db(&mut conn).await?;

    let mut client = OhmeClient::new()
        .authenticate(&username, &password);

    let token_file = path::PathBuf::from("data/ohme-token.json");
    if login {
        client.verify_password().await?;
        let _ = client.write_token(&token_file);
    } else {
        client.read_token(&token_file)?;
    }

    let refresh_token_file = path::PathBuf::from("data/ohme-refreshed-token.json");
    if refresh {
        client.refresh_token().await?;
        let _ = client.write_refresh_token(&refresh_token_file);
    } else {
        client.read_refresh_token(&refresh_token_file)?;
    }

    //eprintln!("{:#?}", client);

    let timerange: Option<TimeRange> = if let (Some(from), Some(to)) = (from, to) {
        Some(TimeRange::new_from_strs(&from, &to)?)
    } else {
        None
    };

    let summary = client.get_charge_summary(&user_id, timerange).await?;

    for stats in &summary.stats {
        let time = DateTime::<Utc>::from_timestamp_millis(stats.start_time as i64)
            .ok_or_eyre("not millis")?;
        let charged: f32 = stats.energy_charged_total_wh as f32 / 1000 as f32;
        println!("{time}: {charged} kWh");

        sqlx::query(
            "INSERT OR REPLACE INTO carcharge(
                id,
                timestamp,
                charge
            )

            VALUES (?, ?, ?);"
        )
            .bind(&user_id)
            .bind(&dt2str(time))
            .bind(&charged)
            .execute(&mut *conn)
            .await?;
    }


    Ok(())
}

async fn migrate_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    sqlx::query::<Sqlite>(
        "BEGIN;

        CREATE TABLE IF NOT EXISTS carcharge (
            id        TEXT,
            timestamp TEXT, -- use UTC date arithmetic
            charge    REAL, -- kWh
            PRIMARY KEY (id, timestamp)
        );

        CREATE INDEX IF NOT EXISTS carcharge_timestamp ON carcharge(timestamp);

        COMMIT;"
    )
        .execute(&mut *conn)
        .await?;

    Ok(())
}
