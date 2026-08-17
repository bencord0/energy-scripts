use clap::Parser;
use eyre::Error;
use power::{AppState, migrate};

#[derive(Parser, Debug)]
struct Args {
    #[arg(default_value = "sqlite:data/power.sqlite3")]
    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { db } = Args::parse();

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;

    migrate::migrate(&mut conn).await?;
    println!("schema up to date: {db}");
    Ok(())
}
