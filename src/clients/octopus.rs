use chrono::{
    DateTime,
    Days,
    Utc,
};
use serde::{Serialize, Deserialize};
use std::time::Duration;
use eyre::Error;

const ISO_FORMAT: &str = "%Y-%m-%dT00:00:00Z";


pub struct OctopusClient {
    // TODO: Use a pre-authenticated request::Client
    api_key: String,
}

impl OctopusClient {
    pub fn new() -> Self {
        Self {
            api_key: String::new(),
        }
    }

    pub fn api_key(mut self, key: String) -> Self {
        self.api_key = key;
        self
    }

    pub async fn get_consumption(
        &self, mpan: &str, serial: &str,
        from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>,
    ) -> Result<Consumption, Error>
    {
        let now = Utc::now();
        let today = now.date_naive();
        let daybefore = today.clone() - Days::new(2);
        //let yesterday = today.clone() - Days::new(1);
        let tomorrow = today + Days::new(1);

        let period_from = if let Some(from) = from {
            format!("{}", from.date_naive().format(&ISO_FORMAT))
        } else {
            format!("{}", daybefore.format(&ISO_FORMAT))
        };
        let period_to = if let Some(to) = to {
            format!("{}", to.date_naive().format(&ISO_FORMAT))
        } else {
            format!("{}", tomorrow.format(&ISO_FORMAT))
        };

        assert!(period_from < period_to, "time periods mixed up");
        eprintln!("period_from: {period_from}");
        eprintln!("period_to  : {period_to}");

        let url = format!("https://api.octopus.energy/v1/electricity-meter-points/{mpan}/meters/{serial}/consumption");
        let response = reqwest::Client::new()
            .get(url)
            .basic_auth(&self.api_key, None::<&str>)
            .timeout(Duration::from_secs(5))
            .query(&[
                ("period_from", period_from),
                ("period_to", period_to),
                ("order_by", String::from("period")),
            ])
            .send()
            .await?;

        let consumption: Consumption = response.json().await?;
        eprintln!("CONSUMPTION: {consumption:#?}");
        Ok(consumption)
    }

    pub async fn get_product_unit_rates(&self, product_code: &str, tariff_code: &str)
        -> Result<TariffRates, Error>
    {
        let url = format!("https://api.octopus.energy/v1/products/{product_code}/electricity-tariffs/{tariff_code}/standard-unit-rates/");

        // TODO: implement params = { page: ... }
        // TODO: Reuse client between calls
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        let tariff_rates: TariffRates = response.json().await?;
        eprintln!("TARIFF RATES: {tariff_rates:#?}");
        Ok(tariff_rates)
    }

    pub async fn get_standing_charges(&self, product_code: &str, tariff_code: &str)
        -> Result<StandingCharges, Error>
    {
        let url = format!("https://api.octopus.energy/v1/products/{product_code}/electricity-tariffs/{tariff_code}/standing-charges/");
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        let standing_charges: StandingCharges = response.json().await?;
        eprintln!("STANDING CHARGES: {standing_charges:#?}");
        Ok(standing_charges)
    }

    pub async fn get_account(&self, account_id: &str) -> Result<Account, Error> {
        let url = format!("https://api.octopus.energy/v1/accounts/{account_id}/");
        let response = reqwest::Client::new()
            .get(url)
            .basic_auth(&self.api_key, None::<&str>)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        let account: Account = response.json().await?;
        Ok(account)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Consumption {
    pub count: u16,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Vec<ConsumptionResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConsumptionResult {
    pub consumption: f32,
    pub interval_start: String,
    pub interval_end: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TariffRates {
    pub count: u32,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Vec<TariffRatesResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TariffRatesResult {
    pub value_exc_vat: f32,
    pub value_inc_vat: f32,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub payment_method: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StandingCharges {
    pub count: u16,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub results: Vec<StandingChargesResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StandingChargesResult {
    pub value_exc_vat: f32,
    pub value_inc_vat: f32,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub payment_method: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub number: String,
    pub properties: Vec<AccountProperty>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AccountProperty {
    pub id: u32,
    pub postcode: String,
    pub town: Option<String>,
    pub address_line_1: Option<String>,
    pub address_line_2: Option<String>,
    pub address_line_3: Option<String>,
    pub county: Option<String>,
    pub moved_in_at: Option<String>,
    pub moved_out_at: Option<String>,
    pub electricity_meter_points: Vec<ElectricityMeterPoint>,
    pub gas_meter_points: Vec<serde_json::Value>,
    pub consumption_day: Option<f32>,
    pub consumption_night: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ElectricityMeterPoint {
    pub mpan: String,
    pub profile_class: u32,
    pub is_export: bool,
    pub meters: Vec<Meter>,
    pub agreements: Vec<Agreement>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Meter {
    pub serial_number: String,
    pub registers: Vec<Register>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Register {
    pub identifier: String,
    pub is_settlement_register: bool,
    pub rate: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Agreement {
    pub tariff_code: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
}

