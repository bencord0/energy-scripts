use clap::Parser;
use eyre::Error;
use std::{
    collections::HashMap,
    env,
    fs,
    path::PathBuf,
};
use sqlx::sqlite::{
    Sqlite,
    SqliteConnection,
};
use power::{
    AppState,
    KrakenClient,
    OctopusClient,
    clients::{
        kraken::{KrakenAgreement, MeterPointAgreements, TimeOfUseScheme},
        octopus::Account,
    },
    migrate::{self, SqlType::Text},
    dates::{
        dt2str,
        str2dt,
    },
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    account_id: String,

    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Args { account_id, db } = Args::parse();

    let api_key = env::var("OCTOPUS_API_KEY")?;
    let octopus = OctopusClient::new()
        .api_key(api_key.clone());
    let account = octopus.get_account(&account_id).await?;

    let data_file = PathBuf::from(format!("data/account-{account_id}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&account)?)?;
    println!("Saved account: {data_file:?}");

    let mut kraken = KrakenClient::new(api_key);
    let kraken_agreements = kraken.electricity_agreements(&account_id).await?;

    let data_file = PathBuf::from(format!("data/agreements-{account_id}.json"));
    fs::write(&data_file, serde_json::to_vec_pretty(&kraken_agreements)?)?;
    println!("Saved agreements: {data_file:?}");

    let app = AppState::connect(&db)?;
    let mut conn = app.acquire_sqlite().await?;
    check_db(&mut conn).await?;
    let kraken_agreements = by_start(&kraken_agreements)?;
    save_agreements(&mut conn, &account, &kraken_agreements).await?;

    Ok(())
}

async fn check_db(conn: &mut SqliteConnection) -> Result<(), Error> {
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

/// Keyed by (mpan, valid_from). The REST accounts endpoint returns neither
/// product codes nor time-of-use windows, and tariff codes are opaque.
fn by_start(
    meter_points: &[MeterPointAgreements],
) -> Result<HashMap<(String, String), &KrakenAgreement>, Error> {
    let mut agreements = HashMap::new();
    for meter_point in meter_points {
        for agreement in &meter_point.agreements {
            agreements.insert(
                (meter_point.mpan.clone(), dt2str(str2dt(&agreement.valid_from)?)),
                agreement,
            );
        }
    }
    Ok(agreements)
}

async fn save_agreements(
    conn: &mut SqliteConnection,
    account: &Account,
    kraken_agreements: &HashMap<(String, String), &KrakenAgreement>,
) -> Result<(), Error> {
    let meter_points = account.properties.iter()
        .flat_map(|p| &p.electricity_meter_points);

    for meter_point in meter_points {
        let tariff_type = if meter_point.is_export { "EXPORT" } else { "IMPORT" };

        for agreement in &meter_point.agreements {
            // The API mixes `Z` and `+01:00`; billed_tariff_rates compares strings.
            let valid_from = dt2str(str2dt(&agreement.valid_from)?);

            let kraken = match kraken_agreements.get(&(meter_point.mpan.clone(), valid_from.clone())) {
                Some(kraken) if kraken.tariff.tariff_code == agreement.tariff_code => kraken,
                Some(kraken) => return Err(eyre::eyre!(
                    "{} from {valid_from}: REST has {}, Kraken has {}",
                    meter_point.mpan, agreement.tariff_code, kraken.tariff.tariff_code,
                )),
                None => return Err(eyre::eyre!(
                    "{} from {valid_from}: no Kraken agreement for {}",
                    meter_point.mpan, agreement.tariff_code,
                )),
            };
            let valid_to = agreement.valid_to.as_deref()
                .map(|t| str2dt(t).map(dt2str))
                .transpose()?;

            println!(
                "UPSERT agreement {} {tariff_type} {} {valid_from}..{}",
                meter_point.mpan,
                agreement.tariff_code,
                valid_to.as_deref().unwrap_or(""),
            );
            // valid_to moves when an open-ended agreement is closed by a switch.
            sqlx::query::<Sqlite>(
                "INSERT OR REPLACE INTO agreements(
                    mpan,
                    type,
                    tariff_code,
                    product_code,
                    valid_from,
                    valid_to)
                VALUES(?, ?, ?, ?, ?, ?)"
            )
                .bind(&meter_point.mpan)
                .bind(tariff_type)
                .bind(&agreement.tariff_code)
                .bind(&kraken.tariff.product_code)
                .bind(&valid_from)
                .bind(&valid_to)
                .execute(&mut *conn)
                .await?;

            save_timeslots(conn, &meter_point.mpan, &valid_from, kraken.time_of_use_scheme.as_ref()).await?;
        }
    }

    Ok(())
}

async fn save_timeslots(
    conn: &mut SqliteConnection,
    mpan: &str,
    valid_from: &str,
    scheme: Option<&TimeOfUseScheme>,
) -> Result<(), Error> {
    sqlx::query::<Sqlite>(
        "DELETE FROM agreement_timeslots WHERE mpan = ? AND valid_from = ?"
    )
        .bind(mpan)
        .bind(valid_from)
        .execute(&mut *conn)
        .await?;

    let Some(scheme) = scheme else {
        return Ok(())
    };
    for slot in &scheme.timeslots {
        if !slot.is_every_day() {
            return Err(eyre::eyre!(
                "{mpan} from {valid_from}: {} timeslot {} is restricted to some days",
                scheme.name, slot.timeslot,
            ));
        }
        println!(
            "UPSERT timeslot {mpan} {valid_from} {} {}..{} {}",
            slot.timeslot, slot.active_from, slot.active_to, scheme.timezone,
        );
        sqlx::query::<Sqlite>(
            "INSERT INTO agreement_timeslots(
                mpan,
                valid_from,
                timeslot,
                timezone,
                active_from,
                active_to)
            VALUES(?, ?, ?, ?, ?, ?)"
        )
            .bind(mpan)
            .bind(valid_from)
            .bind(&slot.timeslot)
            .bind(&scheme.timezone)
            .bind(&slot.active_from)
            .bind(&slot.active_to)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
