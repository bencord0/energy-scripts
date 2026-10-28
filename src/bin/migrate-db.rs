use clap::Parser;
use eyre::Error;
use power::migrate;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

#[derive(Parser, Debug)]
struct Args {
    /// Path to the database.
    #[arg(default_value = "data/power.sqlite3")]
    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { db } = Args::parse();
    let path = db.strip_prefix("sqlite:").unwrap_or(&db);

    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);

    let mut conn = SqliteConnection::connect_with(&opts).await?;
    migrate::migrate(&mut conn).await?;

    println!("schema up to date: {path}");
    Ok(())
}
