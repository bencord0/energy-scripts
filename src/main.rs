use axum::{
    extract::{State, Query},
    http::StatusCode,
    routing::get,
    Router,
    Json,
};
use chrono::{
    DateTime,
    Utc,
};
use eyre::{WrapErr, Error};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use tower_http::{
    services::ServeDir,
    trace::TraceLayer as TowerTraceLayer,
};
use power::{
    AppState,
    dates::str2dt,
};

const SQLITE_URL: &'static str = "sqlite:data/power.sqlite3";

#[tokio::main]
async fn main() -> Result<(), Error> {
    // basic logging, stdout
    tracing_subscriber::fmt::init();

    let state = AppState::connect(&SQLITE_URL)?;

    let app = Router::new()
        .route("/hello", get(index))
        .route("/version", get(version))
        .route("/api/consumption", get(consumption))
        .route("/api/consumption-by-time", get(consumption_by_time))
        .route("/api/price-distribution", get(price_distribution))
        .fallback_service(ServeDir::new("./src"))
        .layer(TowerTraceLayer::new_for_http())
        .with_state(Arc::new(state))
        ;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn index() -> &'static str {
    "Hello world!"
}

#[derive(sqlx::FromRow)]
struct Version {
    //key: String,
    value: String,
}

async fn version(State(app): State<Arc<AppState>>)
    -> Result<String, StatusCode>
{
    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let version: Version = sqlx::query_as(
        "SELECT value
         FROM versioning
         WHERE key = 'version'
         LIMIT 1"
    )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("Fetch version: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(version.value)
}

#[derive(Serialize, Debug)]
struct Consumption {
    timestamp: DateTime<Utc>,
    consumption: f32,
    generation: f32,
    rate: f32,
    cost: f32,
    sale: f32,
    charge: f32,
    discharge: f32,
}

#[derive(Deserialize, Debug)]
struct ConsumptionQuery {
    start: String,
    end: String,
    r#type: String,
    window: String,
}

#[axum::debug_handler]
async fn consumption(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionQuery>,
)
    -> Result<Json<Vec<Consumption>>, StatusCode>
{
    let ConsumptionQuery { start, end, r#type, window } = query;
    let mut data: Vec<Consumption> = Vec::new();

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
            strftime('%Y-%m-%dT00:00:00Z', r.valid_from), -- daily
            strftime('%Y-%m-%dT%H:00:00Z', r.valid_from), -- hourly
            r.valid_from,                                 -- half-hourly

            SUM(c.consumption),
            SUM(c.generation),
            AVG(r.value),
            SUM(c.consumption * r.value),
            SUM(c.generation * r.value),
            SUM(b.charge),
            SUM(b.discharge)
        FROM tariff_rates as r

        JOIN products as p
          ON r.product_code = p.product_code
         AND r.tariff_code = p.tariff_code

        LEFT JOIN consumption as c
               ON r.valid_from = c.interval_start

        LEFT JOIN charge as b
               ON r.valid_from = b.start

        WHERE r.valid_from >= ? -- start
          AND r.valid_from < ?  -- end
          AND p.type = ?        -- type

        GROUP BY
            CASE ?              -- window
              WHEN '1d' THEN date(r.valid_from)
              WHEN '1h' THEN strftime('%Y-%m-%dT%H:00:00Z', r.valid_from)
              ELSE r.valid_from
            END

        ORDER BY r.valid_from ASC"
    )
        .bind(&start)
        .bind(&end)
        .bind(&r#type)
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
            rate: row.get(5),
            cost: row.get(6),
            sale: row.get(7),
            charge: row.get(8),
            discharge: row.get(9),
        };
        data.push(consumption);
    }

    Ok(Json(data))
}

#[derive(Serialize, Debug)]
struct PriceDistribution {
    import_rate: f32,
    export_rate: f32,
    consumption: f32,
    generation: f32,
    slots: u32,
    cost: f32,
    sale: f32,
}

#[derive(Deserialize, Debug)]
struct PriceDistributionQuery {
    start: String,
    end: String,
}

async fn price_distribution(
    State(app): State<Arc<AppState>>,
    Query(query): Query<PriceDistributionQuery>,
)
    -> Result<Json<Vec<PriceDistribution>>, StatusCode>
{
    let PriceDistributionQuery { start, end } = query;
    let mut data: Vec<PriceDistribution> = Vec::new();

    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = sqlx::query(
        "SELECT
            import.value as import_rate,
            export.value as export_rate,
            c.consumption as consumption,
            c.generation as generation,
            1 as slots,
            (import.value * c.consumption) as cost,
            (export.value * c.generation) as sale
        FROM consumption as c

        LEFT JOIN tariff_rates as import

        LEFT JOIN tariff_rates as export
        ON
            c.interval_start = import.valid_from
        AND c.interval_start = export.valid_from

        JOIN products as pimport
        ON
            import.product_code = pimport.product_code
        AND import.tariff_code = pimport.tariff_code

        JOIN products as pexport
        ON
            export.product_code = pexport.product_code
        AND export.tariff_code = pexport.tariff_code

        WHERE
            c.interval_start >= ? -- start
        AND c.interval_start <  ? -- end
        AND pimport.type = 'IMPORT'
        AND pexport.type = 'EXPORT';"
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
        let pd = PriceDistribution {
            import_rate: row.get(0),
            export_rate: row.get(1),
            consumption: row.get(2),
            generation: row.get(3),
            slots: row.get(4),
            cost: row.get(5),
            sale: row.get(6),
        };
        data.push(pd);
    }

    Ok(Json(data))
}

#[derive(Serialize, Debug)]
struct ConsumptionByTime {
    time_of_day: String,
    import_rate: f32,
    export_rate: f32,
    consumption: f32,
    generation: f32,
}

#[derive(Deserialize, Debug)]
struct ConsumptionByTimeQuery {
    start: String,
    end: String,
}

#[axum::debug_handler]
async fn consumption_by_time(
    State(app): State<Arc<AppState>>,
    Query(query): Query<ConsumptionByTimeQuery>,
)
    -> Result<Json<Vec<ConsumptionByTime>>, StatusCode>
{
    let ConsumptionByTimeQuery { start, end } = query;
    log::info!("consumption_by_time: {start} - {end}");
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
