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
    collections::HashMap,
    fs,
    path::PathBuf,
};
use power::{
    AppState,
    AgilePredictClient,
    clients::AgilePrediction,
    migrate,
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
    let (import_predictions, export_predictions) = {
        let import_p = predictor.get_import_prediction(&region).await;
        let export_p = predictor.get_export_prediction(&region).await;

        // Ignore timeout when the site is down
        for p in [&import_p, &export_p] {
            if let Err(e) = p {
                if let Some(req_e) = e.downcast_ref::<reqwest::Error>() {
                    if req_e.is_timeout() {
                        eprintln!("{:?}", e);
                        std::process::exit(0);
                    }
                }
            }
        }

        (import_p?, export_p?)
    };

    let data_file = PathBuf::from(format!("data/prediction-{region}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&[&import_predictions, &export_predictions])?)?;

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    let _ = app.acquire_pg().await?;
    let interval = last_interval(
        &mut conn,
        "AGILE-24-10-01",
        "E-1R-AGILE-24-10-01-A",
    ).await?;

    store_predictions(&mut conn, &region, &import_predictions, &export_predictions, interval).await?;

    Ok(())
}

async fn store_predictions(
    conn: &mut SqliteConnection,
    region: &str,
    import: &[AgilePrediction],
    export: &[AgilePrediction],
    prune_before: DateTime<Utc>,
) -> Result<(), Error> {
    // Export predictions arrive as a parallel series;
    // index by timestamp rather than assuming the two series align slot-for-slot.
    let export_by_time: HashMap<&str, f32> = export[0].prices.iter()
        .map(|p| (p.date_time.as_str(), p.agile_pred))
        .collect();

    for slot in &import[0].prices {
        let timestamp = str2dt(&slot.date_time)?;
        let import_prediction = slot.agile_pred;
        let export_prediction = export_by_time.get(slot.date_time.as_str()).copied();

        sqlx::query(
            "INSERT OR REPLACE INTO agile_predictions(
                region,
                timestamp,
                import_prediction,
                export_prediction
            )

            VALUES (?, ?, ?, ?);"
        )
            .bind(region)
            .bind(dt2str(timestamp))
            .bind(import_prediction)
            .bind(export_prediction)
            .execute(&mut *conn)
            .await?;
    }


    sqlx::query(
        "DELETE FROM agile_predictions
        WHERE region = ?
          AND timestamp < ?"
    )
        .bind(region)
        .bind(&dt2str(prune_before))
        .execute(&mut *conn)
        .await?;

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

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(
        conn,
        "agile_predictions",
        &["region", "timestamp", "import_prediction", "export_prediction"],
    ).await?;
    // last_interval reads tariff_rates, populated by the save_tariff_rates binary.
    migrate::require_columns(
        conn,
        "tariff_rates",
        &["product_code", "tariff_code", "valid_from"],
    ).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use power::clients::AgilePredictionPrice;
    use sqlx::Connection;

    fn price(date_time: &str, agile_pred: f32) -> AgilePredictionPrice {
        AgilePredictionPrice {
            date_time: date_time.to_string(),
            agile_pred,
            agile_high: agile_pred,
            agile_low: agile_pred,
        }
    }

    fn series(prices: Vec<AgilePredictionPrice>) -> Vec<AgilePrediction> {
        vec![AgilePrediction {
            name: "test".to_string(),
            created_at: "2026-08-17T00:00:00Z".to_string(),
            prices,
        }]
    }

    #[tokio::test]
    async fn store_predictions_keeps_import_and_export_distinct() -> Result<(), Error> {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await?;
        migrate::migrate(&mut conn).await?;

        // Same slots, different import vs export values; plus one import-only slot.
        let import = series(vec![
            price("2026-08-17T10:00:00Z", 10.0),
            price("2026-08-17T10:30:00Z", 7.0),
        ]);
        let export = series(vec![
            price("2026-08-17T10:00:00Z", -3.0),
            // 10:30 deliberately absent from the export series.
        ]);

        let prune_before = str2dt("2000-01-01T00:00:00Z")?;
        store_predictions(&mut conn, "A", &import, &export, prune_before).await?;

        let rows = sqlx::query::<Sqlite>(
            "SELECT import_prediction, export_prediction
             FROM agile_predictions WHERE region = ? ORDER BY timestamp ASC",
        )
        .bind("A")
        .fetch_all(&mut conn)
        .await?;

        assert_eq!(rows.len(), 2);

        let import0: f32 = rows[0].get(0);
        let export0: Option<f32> = rows[0].get(1);
        assert_eq!(import0, 10.0);
        assert_eq!(export0, Some(-3.0));
        assert_ne!(import0, export0.unwrap()); // guards a swap / same-value regression

        let import1: f32 = rows[1].get(0);
        let export1: Option<f32> = rows[1].get(1);
        assert_eq!(import1, 7.0);
        assert_eq!(export1, None); // missing export slot stays NULL
        Ok(())
    }
}
