use eyre::{Error, OptionExt};
use chrono::{DateTime, Utc};
use md5::{Digest, Md5};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Serialize, Deserialize};
use std::{
    collections::HashMap,
    time::Duration,
};
use crate::dates::str2dt;

pub struct FoxESSClient {
    api_key: String,
}

impl FoxESSClient {
    pub fn new() -> Self {
        Self {
            api_key: String::new(),
        }
    }

    pub fn api_key(mut self, key: String) -> Self {
        self.api_key = key;
        self
    }

    pub async fn get_inverter_history(&self, sn: &str)
        -> Result<InverterHistory, Error>
    {
        let timestamp = Utc::now().timestamp_millis();
        let path = "/op/v0/device/history/query";

        let url = format!("https://www.foxesscloud.com{path}");

        let token = &self.api_key;
        let mut headers = HeaderMap::new();
        headers.insert("token", HeaderValue::from_str(&token)?);
        headers.insert("lang", HeaderValue::from_static("en"));
        headers.insert("timestamp", timestamp.into());

        let signature = {
            let mut hasher = Md5::new();
            let content = format!("{path}\\r\\n{token}\\r\\n{timestamp}");
            hasher.update(content);
            let hash = hasher.finalize();
            hex::encode(hash)
        };
        eprintln!("signature: {}", signature);
        headers.insert("signature", HeaderValue::from_str(&signature)?);

        eprintln!("headers: {:?}", headers);

        let mut payload = HashMap::new();
        payload.insert("sn", sn);

        let response = reqwest::Client::new()
            .post(url)
            .headers(headers)
            .json(&payload)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        let history: InverterHistoryResponse = response.json().await?;

        //eprintln!("InverterHistoryResponse: {:#?}", history);
        // -- Transform

        let mut result = history.result.ok_or_eyre("no result")?[0].clone();

        let generation_data: InverterHistoryResponseResultData = result.datas.extract_if(..,
            |x| x.variable == "generation"
        )
            .collect::<Vec<InverterHistoryResponseResultData>>()[0].clone();

        Ok(InverterHistory{
            serial: result.deviceSN.clone(),
            generation: Generation {
                unit: generation_data.unit.ok_or_eyre("unit")?,
                data: generation_data.data.into_iter().map(|d| GenerationData {
                    time: str2dt(&d.time).expect("fox timestamp"),
                    value: d.value,
                }).collect()
            }
        })
    }

}

#[derive(Deserialize, Debug)]
struct InverterHistoryResponse {
    errno: u32,
    msg: String,
    result: Option<Vec<InverterHistoryResponseResult>>,
}

#[derive(Deserialize, Clone, Debug)]
struct InverterHistoryResponseResult {
    deviceSN: String,
    datas: Vec<InverterHistoryResponseResultData>,
}

#[derive(Deserialize, Clone, Debug)]
struct InverterHistoryResponseResultData {
    variable: String,
    name: String,
    unit: Option<String>,
    data: Vec<InverterHistoryResponseResultDataItem>,
}

#[derive(Deserialize, Clone, Debug)]
struct InverterHistoryResponseResultDataItem {
    time: String,
    value: f32,
}

#[derive(Serialize, Debug)]
pub struct InverterHistory {
    pub serial: String,
    pub generation: Generation,
}

#[derive(Serialize, Debug)]
pub struct Generation {
    pub data: Vec<GenerationData>,
    pub unit: String,
}

#[derive(Serialize, Debug)]
pub struct GenerationData {
    pub time: DateTime<Utc>,
    pub value: f32,
}
