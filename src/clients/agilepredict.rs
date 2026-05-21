use eyre::Error;
use serde::{Serialize, Deserialize};
use std::time::Duration;

pub struct AgilePredictClient{}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgilePrediction {
    pub name: String,
    pub created_at: String,
    pub prices: Vec<AgilePredictionPrice>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgilePredictionPrice {
    pub date_time: String,
    pub agile_pred: f32,
    pub agile_high: f32,
    pub agile_low: f32,
}

impl AgilePredictClient {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn get_prediction(&self, region: &str)
        -> Result<Vec<AgilePrediction>, Error>
    {
        let url = format!("https://agilepredict.com/api/{region}/");
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        let predictions: Vec<AgilePrediction> = response.json().await?;
        Ok(predictions)
    }
}
