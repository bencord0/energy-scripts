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

struct QueryOptions<'a> {
    export: Option<u8>,
    format: Option<&'a str>,
}

impl<'a> QueryOptions<'a> {
    fn new() -> Self {
        QueryOptions {
            export: None,
            format: None,
        }
    }

    fn export(mut self, e: u8) -> Self {
        self.export = Some(e);
        self
    }

    fn format(mut self, j: &'a str) -> Self {
        self.format = Some(j);
        self
    }

    // Consumes `self`.
    fn build_query(self) -> Vec<(&'static str, String)> {
        let mut query = Vec::new();
        if let Some(format) = self.format {
            query.push(("format", format.to_string()));
        }
        if let Some(export) = self.export {
            query.push(("export", export.to_string()));
        }
        query
    }
}

impl AgilePredictClient {
    pub fn new() -> Self {
        Self {}
    }

    async fn get_prediction(&self, region: &str, options: QueryOptions<'_>)
        -> Result<Vec<AgilePrediction>, Error>
    {
        let url = format!("https://agilepredict.com/api/{region}/");
        let response = reqwest::Client::new()
            .get(url)
            .query(&options.build_query())
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        let predictions: Vec<AgilePrediction> = response.json().await?;
        Ok(predictions)
    }

    pub async fn get_import_prediction(&self, region: &str)
        -> Result<Vec<AgilePrediction>, Error>
    {
        let opts = QueryOptions::new()
            .export(0)
            .format("json");

        self.get_prediction(region, opts).await
    }

    pub async fn get_export_prediction(&self, region: &str)
        -> Result<Vec<AgilePrediction>, Error>
    {
        let opts = QueryOptions::new()
            .export(1)
            .format("json");

        self.get_prediction(region, opts).await
    }
}
