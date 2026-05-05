use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Router,
};
use std::sync::Arc;
use sqlx::sqlite::SqlitePool;
use sqlx::postgres::PgPool;
use tower_http::{
    services::ServeDir,
    trace::TraceLayer as TowerTraceLayer,
};

const SQLITE_URL: &'static str = "sqlite:data/power.sqlite3";
const PG_URL: &'static str = "postgres:///power";

#[derive(Debug)]
struct AppState {
    sqlite: SqlitePool,
    pg: PgPool,
}

#[tokio::main]
async fn main() {
    // basic logging, stdout
    tracing_subscriber::fmt::init();

    let sqlite = SqlitePool::connect_lazy(SQLITE_URL)
        .expect("sqlite lazy connect");
    let pg = PgPool::connect_lazy(PG_URL)
        .expect("pg lazy connect");

    let state = AppState{
        sqlite,
        pg,
    };

    let app = Router::new()
        .route("/hello", get(index))
        .route("/version", get(version))
        .fallback_service(ServeDir::new("./src"))
        .layer(TowerTraceLayer::new_for_http())
        .with_state(Arc::new(state))
        ;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> &'static str {
    "Hello world!"
}

#[derive(sqlx::FromRow)]
struct Version {
    //key: String,
    value: String,
}

async fn version(State(app): State<Arc<AppState>>) -> Result<String, StatusCode> {
    let version: Version = sqlx::query_as(
        "SELECT value
         FROM versioning
         WHERE key = 'version'
         LIMIT 1"
    )
        .fetch_one(&app.sqlite)
        .await
        .map_err(|e| {
            log::error!("Fetch version: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(version.value)
}
