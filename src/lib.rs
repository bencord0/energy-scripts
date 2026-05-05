use std::{
    time::Duration,
};
use serde::{Serialize, Deserialize};
use eyre::Error;
use chrono::{
    DateTime,
    Days,
    Utc,
};

const ISO_FORMAT: &str = "%Y-%m-%dT00:00Z";

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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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
