use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Router,
};
use eyre::{WrapErr, Error};
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

#[tokio::main]
async fn main() -> Result<(), Error> {
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
