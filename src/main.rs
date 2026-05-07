use clap::Parser;
use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Router,
    Json,
};
use eyre::{WrapErr, Error};
use sqlx::Row;
use serde::Serialize;
use std::sync::Arc;
use tower_http::{
    services::ServeDir,
    trace::TraceLayer as TowerTraceLayer,
};
use power::{
    AppState,
    api,
};

const SQLITE_URL: &'static str = "sqlite:data/power.sqlite3";

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    tcp: Option<String>,

    #[arg(long)]
    unix: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();

    // basic logging, stdout
    tracing_subscriber::fmt::init();

    let state = AppState::connect(&SQLITE_URL)?;

    let app = Router::new()
        .route("/hello", get(index))
        .route("/version", get(version))
        .route("/api/consumption", get(api::consumption))
        .route("/api/consumption-by-time", get(api::consumption_by_time))
        .route("/api/price-distribution", get(api::price_distribution))
        .route("/api/standing-charge", get(api::standing_charge))
        .route("/api/agile-prediction", get(api::agile_prediction))
        .route("/api/data-limits", get(data_limits))
        .fallback_service(ServeDir::new("./src"))
        .layer(TowerTraceLayer::new_for_http())
        .with_state(Arc::new(state))
        ;

    // JoinSet cancels all tasks when dropped
    let mut set = tokio::task::JoinSet::new();

    if let Some(tcp) = args.tcp {
        let tcplistener = tokio::net::TcpListener::bind(tcp).await?;
        log::info!("Listening on {tcplistener:?}");
        set.spawn(axum::serve(tcplistener, app.clone()).into_future());
    }

    if let Some(unix) = args.unix {
        let _ = tokio::fs::remove_file(&unix).await?;
        let unixlistener = tokio::net::UnixListener::bind(&unix)?;
        log::info!("Listening on {unixlistener:?}");
        set.spawn(axum::serve(unixlistener, app).into_future());
    }

    while let Some(res) = set.join_next().await {
        // propagate errors
        let _ = res.unwrap();
    }

    log::info!("Exiting");
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
struct DataLimit {
    earliestDate: String,
    latestDate: String,
}

async fn data_limits(State(app): State<Arc<AppState>>)
    -> Result<Json<DataLimit>, StatusCode>
{
    let mut conn = app
        .acquire_sqlite()
        .await
        .wrap_err("acquire sqlite")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let row = sqlx::query(
        "SELECT
            MIN(interval_start),
            MAX(interval_start)

         FROM consumption"
    )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| {
            log::error!("Fetch data limit: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let limit = DataLimit {
            earliestDate: row.get(0),
            latestDate: row.get(1),
        };

    Ok(Json(limit))
}
