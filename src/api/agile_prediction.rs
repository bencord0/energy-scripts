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
    prediction: f32,
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

    let Some(idx) = ["1d", "1h", "30m"].iter().position(|i| *i == &window) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "SELECT
            strftime('%Y-%m-%dT00:00:00Z', timestamp), -- 1d
            strftime('%Y-%m-%dT%H:00:00Z', timestamp), -- 1h
            timestamp,                                 -- 30m
            AVG(prediction)

        FROM agile_predictions

        WHERE region    =  ? -- region
          AND timestamp >= ? -- start
          AND timestamp <  ? -- end

        GROUP BY
          CASE ?             -- window
          WHEN '1d' THEN strftime('%Y-%m-%dT00:00:00Z', timestamp)
          WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', timestamp)
          ELSE timestamp
        END

        ORDER BY timestamp ASC"
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
            timestamp: row.get(idx),
            prediction: row.get(3),
        };
        data.push(p);
    }

    Ok(Json(data))
}
