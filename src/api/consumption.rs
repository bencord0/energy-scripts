use axum::{
    extract::{State, Query},
    http::StatusCode,
    Json,
};
use chrono::{
    DateTime,
    Utc,
};
use eyre::WrapErr;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use crate::{
    AppState,
    dates::str2dt,
};

#[derive(Serialize, Debug)]
pub struct Consumption {
    timestamp: DateTime<Utc>,
    consumption: f32,
    generation: f32,
    import_rate: f32,
    export_rate: f32,
    cost: f32,
    sale: f32,
    charge: f32,
    discharge: f32,
}

#[derive(Deserialize, Debug)]
pub struct ConsumptionQuery {
    start: String,
    end: String,
    window: Option<String>,
}

#[axum::debug_handler]
pub async fn consumption(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionQuery>,
)
    -> Result<Json<Vec<Consumption>>, StatusCode>
{
    let ConsumptionQuery { start, end, window } = query;
    let mut data: Vec<Consumption> = Vec::new();

    let window: String = window.unwrap_or(String::from("30m"));

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
            strftime('%Y-%m-%dT00:00:00Z', i.valid_from), -- daily
            strftime('%Y-%m-%dT%H:00:00Z', i.valid_from), -- hourly
            i.valid_from,                                 -- half-hourly

            SUM(c.consumption),
            SUM(c.generation),
            AVG(i.value),
            AVG(e.value),
            SUM(c.consumption * i.value),
            SUM(c.generation * e.value),
            SUM(b.charge),
            SUM(b.discharge)
        FROM tariff_rates as i

        LEFT JOIN tariff_rates as e
               ON i.valid_from = e.valid_from

        LEFT JOIN consumption as c
               ON c.interval_start = i.valid_from

        JOIN products as pi
          ON i.product_code = pi.product_code
         AND i.tariff_code = pi.tariff_code

        JOIN products as pe
          ON e.product_code = pe.product_code
         AND e.tariff_code = pe.tariff_code

        LEFT JOIN charge as b
               ON i.valid_from = b.start

        WHERE i.valid_from >= ? -- start
          AND i.valid_from < ?  -- end

          AND pi.type = 'IMPORT'
          AND pe.type = 'EXPORT'

        GROUP BY
            CASE ?              -- window
              WHEN '1d' THEN date(i.valid_from)
              WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', i.valid_from)
              ELSE i.valid_from
            END

        ORDER BY i.valid_from ASC"
    )
        .bind(&start)
        .bind(&end)
        .bind(&window)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let consumption = Consumption {
            timestamp: str2dt(row.get(idx))
                .wrap_err("timestamp")
                .map_err(|e| {
                    log::error!("timestamp: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
            consumption: row.get(3),
            generation: row.get(4),
            import_rate: row.get(5),
            export_rate: row.get(6),
            cost: row.get(7),
            sale: row.get(8),
            charge: row.get(9),
            discharge: row.get(10),
        };
        data.push(consumption);
    }

    Ok(Json(data))
}

#[derive(Serialize, Debug)]
pub struct ConsumptionByTime {
    time_of_day: String,
    import_rate: f32,
    export_rate: f32,
    consumption: f32,
    generation: f32,
}

#[derive(Deserialize, Debug)]
pub struct ConsumptionByTimeQuery {
    start: String,
    end: String,
}

#[axum::debug_handler]
pub async fn consumption_by_time(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionByTimeQuery>,
)
    -> Result<Json<Vec<ConsumptionByTime>>, StatusCode>
{
    let ConsumptionByTimeQuery { start, end } = query;
    let mut data: Vec<ConsumptionByTime> = Vec::new();

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "SELECT
            strftime('%H:%M', c.interval_start) as t_30m,
            import.value as import_rate,
            export.value as export_rate,
            SUM(c.consumption) as consumption,
            SUM(c.generation) as generation
        FROM consumption as c

        LEFT JOIN tariff_rates as import
               ON c.interval_start = import.valid_from

        LEFT JOIN tariff_rates as export
               ON c.interval_start = export.valid_from

             JOIN products as pimport
               ON import.product_code = pimport.product_code
              AND import.tariff_code  = pimport.tariff_code

             JOIN products as pexport
               ON export.product_code = pexport.product_code
              AND export.tariff_code  = pexport.tariff_code

        WHERE
            c.interval_start >= ? -- start
        AND c.interval_start <  ? -- end
        AND pimport.type = 'IMPORT'
        AND pexport.type = 'EXPORT'

        GROUP BY t_30m, import_rate
        ORDER BY t_30m ASC, import_rate DESC;"
    )
        .bind(&start)
        .bind(&end)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for row in result {
        let c = ConsumptionByTime {
            time_of_day: row.get(0),
            import_rate: row.get(1),
            export_rate: row.get(2),
            consumption: row.get(3),
            generation: row.get(4),
        };
        data.push(c);
    }

    Ok(Json(data))
}
