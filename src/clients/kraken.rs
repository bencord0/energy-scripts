// Kraken GraphQL client for account data the REST API doesn't expose.
//
// The Saving Sessions schema is undocumented; field names follow
// https://github.com/BottlecapDave/HomeAssistant-OctopusEnergy/blob/develop/custom_components/octopus_energy/api_client/__init__.py
// Reference for the documented parts: https://developer.octopus.energy/graphql/
use eyre::{Error, OptionExt, eyre};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::time::Duration;

const KRAKEN_URL: &str = "https://api.octopus.energy/v1/graphql/";
const BACKEND_URL: &str = "https://api.backend.octopus.energy/v1/graphql/";

pub struct KrakenClient {
    http: reqwest::Client,
    api_key: String,
    token: Option<String>,
}

impl KrakenClient {
    pub fn new(api_key: String) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("power.condi.me/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static client config");
        Self {
            http,
            api_key,
            token: None,
        }
    }

    pub async fn obtain_token(&mut self) -> Result<String, Error> {
        if let Some(token) = &self.token {
            return Ok(token.clone());
        }

        let data: ObtainToken = self
            .post(
                KRAKEN_URL,
                "mutation ObtainToken($apiKey: String!) {
                    obtainKrakenToken(input: { APIKey: $apiKey }) {
                        token
                    }
                }",
                json!({ "apiKey": self.api_key }),
                None,
            )
            .await?;
        let token = data.obtain_kraken_token.token;
        self.token = Some(token.clone());
        Ok(token)
    }

    pub async fn saving_sessions(&mut self, account_number: &str) -> Result<SavingSessions, Error> {
        let token = self.obtain_token().await?;
        let data: SavingSessionsData = self
            .post(
                BACKEND_URL,
                "query SavingSessions($accountNumber: String!) {
                    savingSessions {
                        events(includeDev: false) {
                            id
                            code
                            eventType
                            startAt
                            endAt
                        }
                        account(accountNumber: $accountNumber) {
                            hasJoinedCampaign
                            joinedEvents {
                                eventId
                                eventType
                                startAt
                                endAt
                                rewardGivenInOctoPoints
                            }
                        }
                    }
                }",
                json!({ "accountNumber": account_number }),
                Some(&token),
            )
            .await?;
        Ok(data.saving_sessions)
    }

    async fn post<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &str,
        variables: Value,
        token: Option<&str>,
    ) -> Result<T, Error> {
        let mut request = self.http
            .post(url)
            .timeout(Duration::from_secs(15))
            .json(&json!({ "query": query, "variables": variables }));
        // Raw token, no "JWT " prefix; both gateways accept it.
        if let Some(token) = token {
            request = request.header("Authorization", token);
        }

        // Gateways answer some failures with HTML, so keep the body for the error.
        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;
        let parsed: GraphQLResponse<T> = serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(300).collect();
            eyre!("{url} returned {status}, not GraphQL JSON ({e}): {snippet}")
        })?;
        parsed.into_result()
    }
}

#[derive(Deserialize, Debug)]
struct GraphQLResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQLError>,
}

#[derive(Deserialize, Debug)]
struct GraphQLError {
    message: String,
}

impl<T> GraphQLResponse<T> {
    fn into_result(self) -> Result<T, Error> {
        if !self.errors.is_empty() {
            let messages: Vec<String> = self.errors.into_iter().map(|e| e.message).collect();
            return Err(eyre!("graphql: {}", messages.join("; ")));
        }
        self.data.ok_or_eyre("graphql: response has neither data nor errors")
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ObtainToken {
    obtain_kraken_token: Token,
}

#[derive(Deserialize, Debug)]
struct Token {
    token: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SavingSessionsData {
    saving_sessions: SavingSessions,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavingSessions {
    #[serde(default)]
    pub events: Vec<SavingSessionEvent>,
    pub account: Option<SavingSessionsAccount>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavingSessionEvent {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    pub code: Option<String>,
    // Kept as a string: Octopus adds new types (TURN_UP, WEEKEND_HAPPY_HOUR, ...).
    pub event_type: Option<String>,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavingSessionsAccount {
    pub has_joined_campaign: bool,
    #[serde(default)]
    pub joined_events: Vec<JoinedEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JoinedEvent {
    #[serde(deserialize_with = "id_string")]
    pub event_id: String,
    pub event_type: Option<String>,
    pub start_at: String,
    pub end_at: String,
    /// `None` until the session has been settled.
    pub reward_given_in_octo_points: Option<i64>,
}

/// GraphQL `ID`s may be serialised as either strings or integers.
fn id_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!("unexpected id: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_sessions_response_deserializes() -> Result<(), Error> {
        let body = r#"{
            "data": {
                "savingSessions": {
                    "events": [
                        {"id": 2601, "code": "EVENT_2601", "eventType": "TURN_DOWN",
                         "startAt": "2026-11-17T17:00:00+00:00", "endAt": "2026-11-17T18:00:00+00:00"},
                        {"id": "2602", "code": "HH_2602", "eventType": "WEEKEND_HAPPY_HOUR",
                         "startAt": "2026-11-22T11:00:00+00:00", "endAt": "2026-11-22T12:00:00+00:00"}
                    ],
                    "account": {
                        "hasJoinedCampaign": true,
                        "joinedEvents": [
                            {"eventId": 2601, "eventType": "TURN_DOWN",
                             "startAt": "2026-11-17T17:00:00+00:00", "endAt": "2026-11-17T18:00:00+00:00",
                             "rewardGivenInOctoPoints": 800},
                            {"eventId": 2602, "eventType": "WEEKEND_HAPPY_HOUR",
                             "startAt": "2026-11-22T11:00:00+00:00", "endAt": "2026-11-22T12:00:00+00:00",
                             "rewardGivenInOctoPoints": null},
                            {"eventId": 2603, "eventType": "SOMETHING_NEW",
                             "startAt": "2026-11-23T11:00:00+00:00", "endAt": "2026-11-23T12:00:00+00:00"}
                        ]
                    }
                }
            }
        }"#;

        let response: GraphQLResponse<SavingSessionsData> = serde_json::from_str(body)?;
        let sessions = response.into_result()?.saving_sessions;

        assert_eq!(sessions.events[0].id, "2601", "integer ids are stringified");
        assert_eq!(sessions.events[1].id, "2602", "string ids pass through");

        let joined = sessions.account.unwrap().joined_events;
        assert_eq!(joined.len(), 3);
        assert_eq!(joined[0].reward_given_in_octo_points, Some(800));
        assert_eq!(joined[1].reward_given_in_octo_points, None, "unsettled reward is null");
        assert_eq!(joined[2].event_type.as_deref(), Some("SOMETHING_NEW"), "unknown types still deserialize");
        assert_eq!(joined[2].reward_given_in_octo_points, None, "missing reward field is tolerated");
        Ok(())
    }

    #[test]
    fn graphql_errors_become_eyre_errors() {
        let body = r#"{
            "data": null,
            "errors": [
                {"message": "Unauthorized.", "extensions": {"errorCode": "KT-CT-1111"}},
                {"message": "Second problem."}
            ]
        }"#;

        let response: GraphQLResponse<SavingSessionsData> = serde_json::from_str(body).unwrap();
        let err = response.into_result().unwrap_err().to_string();
        assert!(err.contains("Unauthorized.") && err.contains("Second problem."), "{err}");
    }

    #[test]
    fn errors_take_precedence_over_partial_data() {
        let body = r#"{
            "data": {"obtainKrakenToken": {"token": "t"}},
            "errors": [{"message": "partial failure"}]
        }"#;

        let response: GraphQLResponse<ObtainToken> = serde_json::from_str(body).unwrap();
        assert!(response.into_result().is_err());
    }
}
