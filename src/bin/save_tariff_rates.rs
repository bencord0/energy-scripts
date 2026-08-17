use clap::Parser;
use chrono::{
    DateTime,
    Utc,
};
use eyre::Error;
use std::{
    fs,
    path::PathBuf,
};
use phf::{
    Map,
    phf_map,
};
use sqlx::{
    sqlite::{
        Sqlite,
        SqliteConnection,
    },
    Row,
};

#[derive(Parser,Debug)]
struct Args {
    #[arg(long)]
    product_code: String,
    #[arg(long)]
    tariff_code: String, // contains the region

    db: String,
}

use power::{
    AppState,
    OctopusClient,
    migrate,
    dates::{
        dt2str,
        str2dt,
    },
};

#[derive(Debug, Clone)]
pub enum ImportExport {
    Import,
    Export,
}

static PRODUCTS: Map<&'static str, ImportExport> = phf_map! {
    "VAR-22-11-01" => ImportExport::Import,
    "AGILE-24-10-01" => ImportExport::Import,
    "OUTGOING-VAR-24-10-26" => ImportExport::Export,
    "AGILE-OUTGOING-19-05-13" => ImportExport::Export,
};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { product_code, tariff_code, db } = Args::parse();

    assert!(PRODUCTS.contains_key(&product_code));
    let tariff_type = format!("{:?}", PRODUCTS.get(&product_code).unwrap())
        .to_uppercase();
    println!("tariff_type: {tariff_type}");

    // Public Client, no authentication
    let octopus = OctopusClient::new();

    // Unit Rates
    let unit_rates = octopus.get_product_unit_rates(&product_code, &tariff_code).await?;
    let unit_data_file = PathBuf::from(format!("data/products-{product_code}-{tariff_code}-unit-rates.json"));
    fs::write(&unit_data_file, serde_json::to_vec_pretty(&unit_rates)?)?;
    println!("Saved unit rates: {unit_data_file:?}");

    // Standing Charges
    let standing_charges = octopus.get_standing_charges(&product_code, &tariff_code).await?;
    let standing_data_file = PathBuf::from(format!("data/products-{product_code}-{tariff_code}-standing-charges.json"));
    fs::write(&standing_data_file, serde_json::to_vec_pretty(&standing_charges)?)?;
    println!("Saved standing charges: {standing_data_file:?}");

    // Persist to database
    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    let _ = app.acquire_pg().await?;
    let interval = last_interval(&mut conn, &product_code, &tariff_code).await?;

    let mut standing_charge: f32 = 0.0;
    for rate in standing_charges.results {
        // TODO: Check time range is valid
        standing_charge = rate.value_inc_vat;
        break
    }
    println!("Using standing charge: {standing_charge} p/day");

    // Insert/update the product mapping
    sqlx::query::<Sqlite>(
        "INSERT OR REPLACE INTO products(
            product_code,
            tariff_code,
            type,
            standing_charge)
        VALUES($1, $2, $3, $4)"
    )
        .bind(&product_code)
        .bind(&tariff_code)
        .bind(&tariff_type)
        .bind(&standing_charge)
        .execute(&mut *conn)
        .await?;

    for rate in unit_rates.results {
        let valid_from = str2dt(&rate.valid_from)?;
        let valid_to = rate.valid_to.map(|t| str2dt(&t).unwrap());
        let value = rate.value_inc_vat;

        if valid_from <= interval {
            continue
        }

        println!("INSERT tariff_rate for {product_code} at {valid_from}...");
        sqlx::query::<Sqlite>(
            "INSERT INTO tariff_rates(
                product_code,
                tariff_code,
                valid_from,
                valid_to,
                value,
                daily_standing_charge)
            VALUES(?, ?, ?, ?, ?, ?)"
        )
            .bind(&product_code)
            .bind(&tariff_code)
            .bind(dt2str(valid_from))
            .bind(valid_to.map(|t| dt2str(t)))
            .bind(&value)
            .bind(&standing_charge)
            .execute(&mut *conn)
            .await?;
    }

    Ok(())
}

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(
        conn,
        "products",
        &["product_code", "tariff_code", "type", "standing_charge"],
    ).await?;
    migrate::require_columns(
        conn,
        "tariff_rates",
        &["product_code", "tariff_code", "valid_from", "valid_to", "value", "daily_standing_charge"],
    ).await
}

async fn last_interval(
    conn: &mut SqliteConnection,
    product_code: &str,
    tariff_code: &str,
) -> Result<DateTime<Utc>, Error> {
    let rows = sqlx::query::<Sqlite>(
        "SELECT
            valid_from
        FROM tariff_rates
        WHERE
            product_code = ?
            AND tariff_code = ?
        ORDER BY valid_from DESC
        LIMIT 1"
    )
        .bind(&product_code)
        .bind(&tariff_code)
        .fetch_one(&mut *conn)
        .await?;

    let valid_from = rows.get("valid_from");
    println!("valid_from: {valid_from}");
    let last_interval = str2dt(valid_from)?;
    Ok(last_interval)
}
