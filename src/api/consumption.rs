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
    generation: f32, // rename as export
    solar_generation: f32, // rename as generation
    import_rate: f32,
    export_rate: f32,
    cost: f32,
    sale: f32,
    charge: f32,
    discharge: f32,
    car_charge: f32,
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

    ["1d", "1h", "30m"]
        .iter()
        .position(|i| *i == &window)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = sqlx::query(
        "WITH rates AS (
            SELECT
                MIN(i.valid_from) AS valid_from,
                CASE ?3
                    WHEN '1d' THEN date(i.valid_from)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', i.valid_from)
                    ELSE i.valid_from
                END AS slot,
                AVG(CASE WHEN pi.type='IMPORT' THEN i.value END) AS import_value,
                AVG(CASE WHEN pe.type='EXPORT' THEN e.value END) AS export_value

            FROM tariff_rates i

            JOIN tariff_rates e
              ON i.valid_from = e.valid_from

            JOIN products pi
              ON i.product_code = pi.product_code
             AND i.tariff_code = pi.tariff_code

            JOIN products pe
              ON e.product_code = pe.product_code
             AND e.tariff_code = pe.tariff_code

           GROUP BY slot
        ),

        grid AS (
            SELECT
                MIN(c.interval_start) AS interval_start,
                CASE ?3
                    WHEN '1d' THEN date(c.interval_start)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', c.interval_start)
                    ELSE c.interval_start
                END AS slot,
                SUM(consumption) AS consumption,
                SUM(generation) AS generation

            FROM consumption c
            GROUP BY slot
        ),

        charge_diff AS (
            SELECT
                MIN(c.start) AS start,
                CASE ?3
                    WHEN '1d' THEN date(c.start)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', c.start)
                    ELSE c.start
                END AS slot,

                SUM(c.charge) AS charge,
                SUM(c.discharge) AS discharge

            FROM charge c
            GROUP BY slot
        ),

        solar_sum AS (
            SELECT
                MIN(s.timestamp) AS timestamp,
                CASE ?3
                    WHEN '1d' THEN date(s.timestamp)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', s.timestamp)
                    ELSE s.timestamp
                END AS slot,
                SUM(value) AS solar

            FROM solar_generation s
            GROUP BY slot
        ),

        car_sum AS (
            SELECT
                MIN(c.timestamp) AS timestamp,
                CASE ?3
                    WHEN '1d' THEN date(c.timestamp)
                    WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', c.timestamp)
                    ELSE c.timestamp
                END AS slot,
                SUM(charge) AS car_charge

            FROM carcharge c
            GROUP BY slot
        )

        SELECT
            r.slot,

            grid.consumption,
            grid.generation,

            r.import_value,
            r.export_value,

            grid.consumption * r.import_value AS cost,
            grid.generation * r.export_value AS sale,

            charge_diff.charge,
            charge_diff.discharge,
            solar_sum.solar,
            car_sum.car_charge

        FROM rates r
        LEFT JOIN grid ON grid.slot = r.slot
        LEFT JOIN charge_diff ON charge_diff.slot = r.slot
        LEFT JOIN solar_sum ON solar_sum.slot = r.slot
        LEFT JOIN car_sum ON car_sum.slot = r.slot
        WHERE r.valid_from >= ?1 AND r.valid_from < ?2

        GROUP BY r.slot
        ORDER BY r.valid_from ASC"
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
            timestamp: str2dt(row.get(0))
                .wrap_err("timestamp")
                .map_err(|e| {
                    log::error!("timestamp: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
            consumption: row.get(1),
            generation: row.get(2),
            import_rate: row.get(3),
            export_rate: row.get(4),
            cost: row.get(5),
            sale: row.get(6),
            charge: row.get(7),
            discharge: row.get(8),
            solar_generation: row.get(9),
            car_charge: row.get(10),
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
