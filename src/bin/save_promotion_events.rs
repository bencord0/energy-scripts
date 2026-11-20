use clap::Parser;
use eyre::Error;
use std::{
    env,
    fs,
    path::PathBuf,
};
use sqlx::sqlite::{
    Sqlite,
    SqliteConnection,
};
use power::{
    AppState,
    KrakenClient,
    clients::kraken::SavingSessions,
    migrate::{self, SqlType::{Integer, Real, Text}},
    dates::{
        dt2str,
        str2dt,
    },
};

// https://octopus.energy/saving-sessions/happy-hours/
const HAPPY_HOUR_FREE_KWH: f32 = 16.0;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    account_id: String,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { account_id, db } = Args::parse();

    let mut kraken = KrakenClient::new(env::var("OCTOPUS_API_KEY")?);
    let sessions = kraken.saving_sessions(&account_id).await?;

    let data_file = PathBuf::from(format!("data/saving-sessions-{account_id}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&sessions)?)?;
    println!("Saved saving sessions: {data_file:?}");

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    save_events(&mut conn, &account_id, &sessions).await?;

    Ok(())
}

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(
        conn,
        "promotion_events",
        &[
            ("account", Text),
            ("event_id", Text),
            ("event_type", Text),
            ("code", Text),
            ("start_at", Text),
            ("end_at", Text),
            ("reward_octopoints", Integer),
            ("free_import_kwh_cap", Real),
        ],
    )
    .await
}

async fn save_events(
    conn: &mut SqliteConnection,
    account_id: &str,
    sessions: &SavingSessions,
) -> Result<(), Error> {
    let Some(account) = &sessions.account else {
        println!("Account {account_id} has not joined Saving Sessions");
        return Ok(());
    };

    for joined in &account.joined_events {
        // joinedEvents omits the code; the campaign event list carries it.
        let event = sessions.events.iter().find(|e| e.id == joined.event_id);
        let event_type = joined.event_type.clone()
            .or_else(|| event.and_then(|e| e.event_type.clone()))
            .unwrap_or_else(|| String::from("UNKNOWN"));
        let free_import_kwh_cap = (event_type == "WEEKEND_HAPPY_HOUR")
            .then_some(HAPPY_HOUR_FREE_KWH);

        // Normalised to the `...Z` form the consumption query compares against.
        let start_at = dt2str(str2dt(&joined.start_at)?);
        let end_at = dt2str(str2dt(&joined.end_at)?);

        println!("UPSERT promotion_event {} {event_type} {start_at}..{end_at}", joined.event_id);
        // Rewards settle days after the event, so later runs overwrite the NULL.
        sqlx::query::<Sqlite>(
            "INSERT OR REPLACE INTO promotion_events(
                account,
                event_id,
                event_type,
                code,
                start_at,
                end_at,
                reward_octopoints,
                free_import_kwh_cap)
            VALUES(?, ?, ?, ?, ?, ?, ?, ?)"
        )
            .bind(account_id)
            .bind(&joined.event_id)
            .bind(&event_type)
            .bind(event.and_then(|e| e.code.as_deref()))
            .bind(&start_at)
            .bind(&end_at)
            .bind(joined.reward_given_in_octo_points)
            .bind(free_import_kwh_cap)
            .execute(&mut *conn)
            .await?;
    }

    Ok(())
}
