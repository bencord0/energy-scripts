use clap::Parser;
use chrono::{
    DateTime,
    Days,
    Utc,
};
use eyre::{Error, OptionExt};
use std::{
    fs,
    path::PathBuf,
};
use phf::{
    Map,
    phf_map,
};
use sqlx::{
    Connection,
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

    /// Re-derive slots from this date; defaults to two days ago so completed
    /// dispatches and corrected prices re-price recent slots.
    #[arg(long)]
    since: Option<String>,

    db: String,
}

use power::{
    AppState,
    OctopusClient,
    clients::octopus::{StandingCharges, TariffRates, UnitRateKind},
    migrate::{self, SqlType::{Real, Text}},
    dates::{
        dt2str,
        str2dt,
    },
    tariffs::{self, Rate, Slot, Timeslot},
    times::TimeRange,
};

#[derive(Debug, Clone)]
pub enum ImportExport {
    Import,
    Export,
}

static PRODUCTS: Map<&'static str, ImportExport> = phf_map! {
    "VAR-22-11-01" => ImportExport::Import,
    "AGILE-24-10-01" => ImportExport::Import,
    "IOG-SMB-VAR-26-10-29" => ImportExport::Import,
    "IOG-SMB-VAR-26-11-04" => ImportExport::Import,
    "GO-VAR-22-10-14" => ImportExport::Import,
    "COSY-22-12-08" => ImportExport::Import,
    "FLUX-IMPORT-23-02-14" => ImportExport::Import,
    "INTELLI-FLUX-IMPORT-23-07-14" => ImportExport::Import,
    "OUTGOING-VAR-24-10-26" => ImportExport::Export,
    "AGILE-OUTGOING-19-05-13" => ImportExport::Export,
    "FLUX-EXPORT-23-02-14" => ImportExport::Export,
};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { product_code, tariff_code, since, db } = Args::parse();

    // Public Client, no authentication
    let octopus = OctopusClient::new();

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    let _ = app.acquire_pg().await?;

    let since = since.map(|s| str2dt(&s)).transpose()?;
    save_rates(&octopus, &mut conn, &product_code, &tariff_code, since).await
}

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
    migrate::require_columns(
        conn,
        "products",
        &[
            ("product_code", Text),
            ("tariff_code", Text),
            ("type", Text),
            ("standing_charge", Real),
        ],
    )
    .await?;
    migrate::require_columns(
        conn,
        "tariff_rates",
        &[
            ("product_code", Text),
            ("tariff_code", Text),
            ("valid_from", Text),
            ("valid_to", Text),
            ("value", Real),
            ("daily_standing_charge", Real),
        ],
    )
    .await?;
    migrate::require_columns(
        conn,
        "agreements",
        &[
            ("mpan", Text),
            ("type", Text),
            ("tariff_code", Text),
            ("product_code", Text),
            ("valid_from", Text),
            ("valid_to", Text),
        ],
    )
    .await?;
    migrate::require_columns(
        conn,
        "intelligent_dispatches",
        &[
            ("start_at", Text),
            ("end_at", Text),
            ("source", Text),
            ("status", Text),
        ],
    )
    .await?;
    migrate::require_columns(
        conn,
        "agreement_timeslots",
        &[
            ("mpan", Text),
            ("valid_from", Text),
            ("timeslot", Text),
            ("timezone", Text),
            ("active_from", Text),
            ("active_to", Text),
        ],
    )
    .await
}

/// Slots from `since` (default two days ago) to the end of tomorrow.
async fn save_rates(
    octopus: &OctopusClient,
    conn: &mut SqliteConnection,
    product_code: &str,
    tariff_code: &str,
    since: Option<DateTime<Utc>>,
) -> Result<(), Error> {
    let tariff_type = format!("{:?}", PRODUCTS.get(product_code).ok_or_eyre("unknown product")?)
        .to_uppercase();
    println!("tariff_type: {tariff_type}");

    let now = Utc::now();
    let window = TimeRange::new(since.unwrap_or(now - Days::new(2)), tariffs::end_of_tomorrow(now))?;

    let standing_charges = fetch_standing_charges(octopus, product_code, tariff_code).await?;
    let standard = octopus
        .get_unit_rates(UnitRateKind::Standard, product_code, tariff_code, Some(&window))
        .await?;

    let slots = if !standard.results.is_empty() {
        cache(product_code, tariff_code, "unit-rates", &standard)?;
        tariffs::expand_half_hours(&to_rates(&standard)?, &standing_charges, &window)
    } else {
        // Two-rate tariffs publish day/night instead of standard rates.
        let day = octopus
            .get_unit_rates(UnitRateKind::Day, product_code, tariff_code, Some(&window))
            .await?;
        let night = octopus
            .get_unit_rates(UnitRateKind::Night, product_code, tariff_code, Some(&window))
            .await?;
        cache(product_code, tariff_code, "day-unit-rates", &day)?;
        cache(product_code, tariff_code, "night-unit-rates", &night)?;

        let timeslots = timeslots(conn, tariff_code).await?;
        if timeslots.is_empty() {
            return Err(eyre::eyre!("{tariff_code}: day/night rates but no agreement_timeslots"));
        }
        let dispatches = dispatches(conn, &window, now).await?;
        tariffs::expand_day_night(&to_rates(&day)?, &to_rates(&night)?, &timeslots, &standing_charges, &window, &dispatches)?
    };

    save_slots(conn, product_code, tariff_code, &tariff_type, &standing_charges, &slots).await
}

/// The day/night windows are only published on the account's agreement, so
/// a two-rate tariff needs save_agreements to have run first.
async fn timeslots(
    conn: &mut SqliteConnection,
    tariff_code: &str,
) -> Result<Vec<Timeslot>, Error> {
    let rows = sqlx::query::<Sqlite>(
        "SELECT DISTINCT
            t.timeslot,
            t.timezone,
            t.active_from,
            t.active_to
        FROM agreement_timeslots AS t
        JOIN agreements AS a
          ON a.mpan = t.mpan
         AND a.valid_from = t.valid_from
        WHERE a.tariff_code = ?"
    )
        .bind(tariff_code)
        .fetch_all(&mut *conn)
        .await?;

    rows.iter()
        .map(|row| Timeslot::new(
            row.get("timeslot"),
            row.get("timezone"),
            row.get("active_from"),
            row.get("active_to"),
        ))
        .collect()
}

/// Smart-charge dispatches overlapping `window`. Planned ones only count for
/// slots not yet started; later runs re-price them once completed (or not).
async fn dispatches(
    conn: &mut SqliteConnection,
    window: &TimeRange,
    now: DateTime<Utc>,
) -> Result<Vec<TimeRange>, Error> {
    let rows = sqlx::query::<Sqlite>(
        "SELECT
            start_at,
            end_at,
            source
        FROM intelligent_dispatches
        WHERE start_at < ?
          AND end_at   > ?
          AND (status = 'completed' OR start_at >= ?)"
    )
        .bind(dt2str(window.end))
        .bind(dt2str(window.start))
        .bind(dt2str(now))
        .fetch_all(&mut *conn)
        .await?;

    rows.iter()
        .filter(|row| tariffs::is_discounted_dispatch(row.get("source")))
        .map(|row| TimeRange::new_from_strs(row.get("start_at"), row.get("end_at")))
        .collect()
}

async fn fetch_standing_charges(
    octopus: &OctopusClient,
    product_code: &str,
    tariff_code: &str,
) -> Result<Vec<Rate>, Error> {
    let standing_charges = octopus.get_standing_charges(product_code, tariff_code).await?;
    cache(product_code, tariff_code, "standing-charges", &standing_charges)?;
    to_standing_charges(&standing_charges)
}

/// Variable tariffs publish a row per payment method for the same period.
fn is_direct_debit(payment_method: Option<&str>) -> bool {
    matches!(payment_method, None | Some("DIRECT_DEBIT"))
}

fn to_rates(rates: &TariffRates) -> Result<Vec<Rate>, Error> {
    rates.results.iter()
        .filter(|r| is_direct_debit(r.payment_method.as_deref()))
        .map(|r| Rate::from_api(r.value_inc_vat, Some(&r.valid_from), r.valid_to.as_deref()))
        .collect()
}

fn to_standing_charges(charges: &StandingCharges) -> Result<Vec<Rate>, Error> {
    charges.results.iter()
        .filter(|r| is_direct_debit(r.payment_method.as_deref()))
        .map(|r| Rate::from_api(r.value_inc_vat, r.valid_from.as_deref(), r.valid_to.as_deref()))
        .collect()
}

fn cache(product_code: &str, tariff_code: &str, kind: &str, data: &impl serde::Serialize) -> Result<(), Error> {
    let data_file = PathBuf::from(format!("data/products-{product_code}-{tariff_code}-{kind}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(data)?)?;
    println!("Saved {kind}: {data_file:?}");
    Ok(())
}

async fn save_slots(
    conn: &mut SqliteConnection,
    product_code: &str,
    tariff_code: &str,
    tariff_type: &str,
    standing_charges: &[Rate],
    slots: &[Slot],
) -> Result<(), Error> {
    let standing_charge = tariffs::rate_at(standing_charges, Utc::now()).unwrap_or_default();
    println!("Using standing charge: {standing_charge} p/day");

    let mut tx = conn.begin().await?;

    // Insert/update the product mapping
    sqlx::query::<Sqlite>(
        "INSERT OR REPLACE INTO products(
            product_code,
            tariff_code,
            type,
            standing_charge)
        VALUES($1, $2, $3, $4)"
    )
        .bind(product_code)
        .bind(tariff_code)
        .bind(tariff_type)
        .bind(standing_charge)
        .execute(&mut *tx)
        .await?;

    // Upsert: dispatches and corrected prices re-price published slots.
    // Standing charges keep the value seen at the time: Agile's endpoint only
    // lists the current charge, backdated to the product launch, so a
    // re-fetch would erase past VAT and price changes.
    for slot in slots {
        sqlx::query::<Sqlite>(
            "INSERT INTO tariff_rates(
                product_code,
                tariff_code,
                valid_from,
                valid_to,
                value,
                daily_standing_charge)
            VALUES(?, ?, ?, ?, ?, ?)
            ON CONFLICT(product_code, tariff_code, valid_from)
            DO UPDATE SET
                valid_to=excluded.valid_to,
                value=excluded.value,
                daily_standing_charge=COALESCE(tariff_rates.daily_standing_charge, excluded.daily_standing_charge)"
        )
            .bind(product_code)
            .bind(tariff_code)
            .bind(dt2str(slot.valid_from))
            .bind(dt2str(slot.valid_to))
            .bind(slot.value)
            .bind(slot.standing_charge)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    match (slots.first(), slots.last()) {
        (Some(first), Some(last)) => println!(
            "UPSERT {} tariff_rates for {product_code}: {}..{}",
            slots.len(), first.valid_from, last.valid_to,
        ),
        _ => println!("No tariff_rates for {product_code}"),
    }
    Ok(())
}
