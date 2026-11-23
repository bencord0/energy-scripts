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
    clients::kraken::{Dispatch, GraphQLErrors},
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

    /// Carry on past upstream failures (errorType APPLICATION).
    #[arg(long)]
    ignore_server_errors: bool,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { account_id, ignore_server_errors, db } = Args::parse();
    let ignore = ignore_server_errors;

    let mut kraken = KrakenClient::new(env::var("OCTOPUS_API_KEY")?);
    let completed = or_none_on_server_error(kraken.completed_dispatches(&account_id).await, ignore, "completedDispatches")?
        .unwrap_or_default();
    // Unknown, not empty: an empty schedule would delete the stored one.
    let planned = or_none_on_server_error(kraken.planned_dispatches(&account_id).await, ignore, "plannedDispatches")?;

    // Cached for comparing schedules against what was billed; not stored.
    let mut flex_planned = Vec::new();
    let devices = or_none_on_server_error(kraken.devices(&account_id).await, ignore, "devices")?;
    for device in devices.unwrap_or_default() {
        let result = kraken.flex_planned_dispatches(&device.id).await;
        let Some(dispatches) = or_none_on_server_error(result, ignore, "flexPlannedDispatches")? else {
            continue;
        };
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
    save_dispatches(&mut conn, &account_id, planned.as_deref(), &completed, Utc::now()).await?;

    Ok(())
}

fn or_none_on_server_error<T>(result: Result<T, Error>, ignore: bool, what: &str) -> Result<Option<T>, Error> {
    match result {
        Err(e) if ignore && e.downcast_ref::<GraphQLErrors>().is_some_and(GraphQLErrors::is_server_error) => {
            eprintln!("Ignoring Kraken server error for {what}: {e}");
            Ok(None)
        }
        other => other.map(Some),
    }
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
    planned: Option<&[Dispatch]>,
    completed: &[Dispatch],
    now: DateTime<Utc>,
) -> Result<(), Error> {
    let mut tx = conn.begin().await?;

    // The schedule is replaced wholesale; planned rows that have already
    // ended are kept, to compare against what was actually billed.
    if planned.is_some() {
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
    } else {
        println!("Keeping stored planned dispatches for {account_id}");
    }

    for (status, dispatches) in [
            ("completed", completed),
            ("planned", planned.unwrap_or_default()),
        ] {
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
        save_dispatches(&mut conn, "A", Some(&[past.clone(), future]), &[], now()).await?;

        // It completes, and the schedule no longer includes the future slot.
        let mut done = past.clone();
        done.delta = Some(-3.2);
        save_dispatches(&mut conn, "A", Some(&[past]), &[done], now()).await?;

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
        save_dispatches(&mut conn, "A", Some(&[past]), &[], now()).await?;
        save_dispatches(&mut conn, "A", Some(&[]), &[], now()).await?;

        assert_eq!(rows(&mut conn).await?.len(), 1, "kept for comparing schedule against billing");
        Ok(())
    }

    #[tokio::test]
    async fn unknown_schedule_keeps_planned_rows() -> Result<(), Error> {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await?;
        migrate::migrate(&mut conn).await?;

        let future = dispatch("2026-11-24T02:00:00+00:00", "2026-11-24T05:00:00+00:00", -20.9);
        save_dispatches(&mut conn, "A", Some(&[future]), &[], now()).await?;
        save_dispatches(&mut conn, "A", None, &[], now()).await?;

        assert_eq!(rows(&mut conn).await?.len(), 1, "a failed fetch is not an empty schedule");
        Ok(())
    }

    fn graphql_error(errors: &str) -> Error {
        GraphQLErrors(serde_json::from_str(errors).unwrap()).into()
    }

    const SERVER_ERROR: &str = r#"[{"message": "Device status could not be fetched.",
        "extensions": {"errorType": "APPLICATION", "errorCode": "KT-CT-4360"}}]"#;
    const AUTH_ERROR: &str = r#"[{"message": "Unauthorized.", "extensions": {"errorCode": "KT-CT-1111"}}]"#;

    #[test]
    fn server_errors_are_ignored_only_when_asked() {
        assert!(or_none_on_server_error::<()>(Err(graphql_error(SERVER_ERROR)), true, "devices").unwrap().is_none());
        assert!(or_none_on_server_error::<()>(Err(graphql_error(SERVER_ERROR)), false, "devices").is_err());
        assert!(or_none_on_server_error::<()>(Err(graphql_error(AUTH_ERROR)), true, "devices").is_err());
        assert_eq!(or_none_on_server_error(Ok(1), true, "devices").unwrap(), Some(1));
    }
}
