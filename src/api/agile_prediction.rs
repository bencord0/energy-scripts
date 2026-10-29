use axum::{
    extract::{State, Query},
    http::StatusCode,
    Json,
};
use eyre::WrapErr;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use crate::AppState;

#[derive(Serialize,Debug)]
pub struct AgilePrediction {
    timestamp: String,
    import_prediction: Option<f32>,
    export_prediction: Option<f32>,
}

#[derive(Deserialize,Debug)]
pub struct AgilePredictionQuery {
    start: String,
    end: String,
    region: String,
    window: String,
}

pub async fn agile_prediction(
    State(app): State<Arc<AppState>>,
    Query(query): Query<AgilePredictionQuery>,
)
    -> Result<Json<Vec<AgilePrediction>>, StatusCode>
{
    let AgilePredictionQuery { start, end, region, window } = query;

    let mut data: Vec<AgilePrediction> = Vec::new();

    ["1w", "1d", "1h", "30m"]
        .iter()
        .position(|i| *i == &window)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "SELECT
            CASE ?4 -- window
                WHEN '1w' THEN strftime('%Y-%m-%dT00:00:00Z', date(timestamp, '-6 days', 'weekday 1'))
                WHEN '1d' THEN strftime('%Y-%m-%dT00:00:00Z', timestamp)
                WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', timestamp)
                ELSE timestamp
            END AS slot,
            AVG(import_prediction),
            AVG(export_prediction)

        FROM agile_predictions

        WHERE region    =  ?1 -- region
          AND timestamp >= ?2 -- start
          AND timestamp <  ?3 -- end

        GROUP BY slot
        ORDER BY slot ASC"
    )
        .bind(&region)
        .bind(&start)
        .bind(&end)
        .bind(&window)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("Fetch all: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let p = AgilePrediction {
            timestamp: row.get(0),
            import_prediction: row.get(1),
            export_prediction: row.get(2),
        };
        data.push(p);
    }

    Ok(Json(data))
}
