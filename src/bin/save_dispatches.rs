use chrono::{
    DateTime,
    Utc,
};
use clap::Parser;
use eyre::Error;
use serde_json::json;
use std::{
    env,
    fs,
    path::PathBuf,
};
use sqlx::{
    Connection,
    sqlite::{
        Sqlite,
        SqliteConnection,
    },
};
use power::{
    AppState,
    KrakenClient,
    clients::kraken::Dispatch,
    migrate::{self, SqlType::{Real, Text}},
    dates::{
        dt2str,
        str2dt,
    },
};

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
    let completed = kraken.completed_dispatches(&account_id).await?;
    let planned = kraken.planned_dispatches(&account_id).await?;

    // Cached for comparing schedules against what was billed; not stored.
    let mut flex_planned = Vec::new();
    for device in kraken.devices(&account_id).await? {
        let dispatches = kraken.flex_planned_dispatches(&device.id).await?;
        flex_planned.push(json!({ "device": device, "planned": dispatches }));
    }

    let data_file = PathBuf::from(format!("data/dispatches-{account_id}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&json!({
        "completed": completed,
        "planned": planned,
        "devices": flex_planned,
    }))?)?;
    println!("Saved dispatches: {data_file:?}");

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    save_dispatches(&mut conn, &account_id, &planned, &completed, Utc::now()).await?;

    Ok(())
}

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(
        conn,
        "intelligent_dispatches",
        &[
            ("account", Text),
            ("start_at", Text),
            ("end_at", Text),
            ("delta_kwh", Real),
            ("source", Text),
            ("location", Text),
            ("status", Text),
        ],
    )
    .await
}

async fn save_dispatches(
    conn: &mut SqliteConnection,
    account_id: &str,
    planned: &[Dispatch],
    completed: &[Dispatch],
    now: DateTime<Utc>,
) -> Result<(), Error> {
    let mut tx = conn.begin().await?;

    // The schedule is replaced wholesale; planned rows that have already
    // ended are kept, to compare against what was actually billed.
    sqlx::query::<Sqlite>(
        "DELETE FROM intelligent_dispatches
        WHERE account = ?
          AND status = 'planned'
          AND end_at > ?"
    )
        .bind(account_id)
        .bind(dt2str(now))
        .execute(&mut *tx)
        .await?;

    for (status, dispatches) in [("planned", planned), ("completed", completed)] {
        if dispatches.is_empty() {
            println!("No {status} dispatches for {account_id}");
        }

        for dispatch in dispatches {
            // Dispatches aren't slot-aligned, so these keep their minutes.
            let start_at = dt2str(str2dt(&dispatch.start)?);
            let end_at = dt2str(str2dt(&dispatch.end)?);
            let meta = dispatch.meta.as_ref();

            println!("UPSERT {status} dispatch {start_at}..{end_at}");
            // A planned dispatch never replaces a completed one.
            sqlx::query::<Sqlite>(
                "INSERT INTO intelligent_dispatches(
                    account,
                    start_at,
                    end_at,
                    delta_kwh,
                    source,
                    location,
                    status)
                VALUES(?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(account, start_at) DO UPDATE SET
                    end_at=excluded.end_at,
                    delta_kwh=excluded.delta_kwh,
                    source=excluded.source,
                    location=excluded.location,
                    status=excluded.status
                WHERE excluded.status = 'completed'
                   OR intelligent_dispatches.status = 'planned'"
            )
                .bind(account_id)
                .bind(&start_at)
                .bind(&end_at)
                .bind(dispatch.delta)
                .bind(meta.and_then(|m| m.source.as_deref()))
                .bind(meta.and_then(|m| m.location.as_deref()))
                .bind(status)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use power::clients::kraken::DispatchMeta;

    fn dispatch(start: &str, end: &str, delta: f64) -> Dispatch {
        Dispatch {
            start: start.to_string(),
            end: end.to_string(),
            delta: Some(delta),
            meta: Some(DispatchMeta { location: None, source: Some(String::from("smart-charge")) }),
        }
    }

    fn now() -> DateTime<Utc> {
        str2dt("2026-11-23T12:00:00Z").unwrap()
    }

    async fn rows(conn: &mut SqliteConnection) -> Result<Vec<(String, String, f64)>, Error> {
        Ok(sqlx::query_as(
            "SELECT start_at, status, delta_kwh FROM intelligent_dispatches ORDER BY start_at"
        ).fetch_all(&mut *conn).await?)
    }

    #[tokio::test]
    async fn completed_replaces_planned_and_schedule_is_refreshed() -> Result<(), Error> {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await?;
        migrate::migrate(&mut conn).await?;

        // One dispatch already ended at `now`, one still scheduled.
        let past = dispatch("2026-11-22T22:00:00+00:00", "2026-11-22T22:30:00+00:00", -3.49);
        let future = dispatch("2026-11-24T02:00:00+00:00", "2026-11-24T05:00:00+00:00", -20.9);
        save_dispatches(&mut conn, "A", &[past.clone(), future], &[], now()).await?;

        // It completes, and the schedule no longer includes the future slot.
        let mut done = past.clone();
        done.delta = Some(-3.2);
        save_dispatches(&mut conn, "A", &[past], &[done], now()).await?;

        assert_eq!(rows(&mut conn).await?, [
            (String::from("2026-11-22T22:00:00Z"), String::from("completed"), -3.2),
        ], "a later planned copy must not demote the completed row; dropped schedule is removed");
        Ok(())
    }

    #[tokio::test]
    async fn ended_planned_dispatches_are_kept() -> Result<(), Error> {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await?;
        migrate::migrate(&mut conn).await?;

        let past = dispatch("2026-11-22T22:00:00+00:00", "2026-11-22T22:30:00+00:00", -3.49);
        save_dispatches(&mut conn, "A", &[past], &[], now()).await?;
        save_dispatches(&mut conn, "A", &[], &[], now()).await?;

        assert_eq!(rows(&mut conn).await?.len(), 1, "kept for comparing schedule against billing");
        Ok(())
    }
}
