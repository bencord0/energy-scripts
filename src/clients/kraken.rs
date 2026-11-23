// Kraken GraphQL client for account data the REST API doesn't expose.
//
// The Saving Sessions and dispatch schemas are undocumented; field names follow
// https://github.com/BottlecapDave/HomeAssistant-OctopusEnergy/blob/develop/custom_components/octopus_energy/api_client/__init__.py
// Reference for the documented parts: https://developer.octopus.energy/graphql/
use eyre::{Error, OptionExt, eyre};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::dates::str2dt;

const KRAKEN_URL: &str = "https://api.octopus.energy/v1/graphql/";
const BACKEND_URL: &str = "https://api.backend.octopus.energy/v1/graphql/";

// "Unable to find device for given account": no smart device is paired.
const NO_DEVICE: &str = "KT-CT-4301";
// "Too many requests": a field-specific limit (rateLimitInfo lists them, e.g.
// flexPlannedDispatches 5/m, obtainKrakenToken 3/m) is exhausted.
const RATE_LIMITED: &str = "KT-CT-1199";
// errorType on internal upstream failures, e.g. KT-CT-4360 "Device status could
// not be fetched", which nulls the whole `devices` list.
const SERVER_ERROR: &str = "APPLICATION";
// Per-minute limits reset within this; hourly ones (completedDispatches
// 300/h) fail rather than stall a cron run.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(120);
// rateLimitInfo needs a token, and both obtainKrakenToken limits are per minute.
const TOKEN_RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);

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

    /// Including inactive agreements; unlike the REST accounts endpoint, each
    /// tariff carries its productCode, and time-of-use tariffs their windows.
    pub async fn electricity_agreements(&mut self, account_number: &str) -> Result<Vec<MeterPointAgreements>, Error> {
        let token = self.obtain_token().await?;
        let data: AgreementsData = self
            .post(
                KRAKEN_URL,
                "query Agreements($accountNumber: String!) {
                    account(accountNumber: $accountNumber) {
                        properties {
                            electricityMeterPoints {
                                mpan
                                agreements(includeInactive: true) {
                                    validFrom
                                    validTo
                                    timeOfUseScheme {
                                        name
                                        timezone
                                        timeslots {
                                            timeslot
                                            activeFrom
                                            activeTo
                                            weekdays
                                            weekends
                                            saturdays
                                            sundays
                                            workdays
                                            holidays
                                        }
                                    }
                                    tariff {
                                        ... on StandardTariff { tariffCode productCode }
                                        ... on DayNightTariff { tariffCode productCode }
                                        ... on ThreeRateTariff { tariffCode productCode }
                                        ... on HalfHourlyTariff { tariffCode productCode }
                                        ... on PrepayTariff { tariffCode productCode }
                                        ... on FourRateEvTariff { tariffCode productCode }
                                    }
                                }
                            }
                        }
                    }
                }",
                json!({ "accountNumber": account_number }),
                Some(&token),
            )
            .await?;
        Ok(data.account.properties.into_iter()
            .flat_map(|p| p.electricity_meter_points)
            .collect())
    }

    pub async fn devices(&mut self, account_number: &str) -> Result<Vec<Device>, Error> {
        let token = self.obtain_token().await?;
        let data: DevicesData = self
            .post(
                KRAKEN_URL,
                "query Devices($accountNumber: String!) {
                    devices(accountNumber: $accountNumber) {
                        id
                        name
                        deviceType
                        provider
                        status { current currentState isSuspended }
                    }
                }",
                json!({ "accountNumber": account_number }),
                Some(&token),
            )
            .await?;
        Ok(data.devices.unwrap_or_default())
    }

    /// Recent history only; callers persist it.
    pub async fn completed_dispatches(&mut self, account_number: &str) -> Result<Vec<Dispatch>, Error> {
        let token = self.obtain_token().await?;
        let data: Result<CompletedDispatchesData, Error> = self
            .post(
                KRAKEN_URL,
                "query CompletedDispatches($accountNumber: String!) {
                    completedDispatches(accountNumber: $accountNumber) {
                        start
                        end
                        delta
                        meta { location source }
                    }
                }",
                json!({ "accountNumber": account_number }),
                Some(&token),
            )
            .await;
        let mut dispatches = or_empty_without_device(data)?.completed_dispatches.unwrap_or_default();
        sort_by_start(&mut dispatches, |d| &d.start)?;
        Ok(dispatches)
    }

    /// Same shape as completed dispatches; only covers the coming schedule.
    pub async fn planned_dispatches(&mut self, account_number: &str) -> Result<Vec<Dispatch>, Error> {
        let token = self.obtain_token().await?;
        let data: Result<PlannedDispatchesData, Error> = self
            .post(
                KRAKEN_URL,
                "query PlannedDispatches($accountNumber: String!) {
                    plannedDispatches(accountNumber: $accountNumber) {
                        start
                        end
                        delta
                        meta { location source }
                    }
                }",
                json!({ "accountNumber": account_number }),
                Some(&token),
            )
            .await;
        let mut dispatches = or_empty_without_device(data)?.planned_dispatches.unwrap_or_default();
        sort_by_start(&mut dispatches, |d| &d.start)?;
        Ok(dispatches)
    }

    pub async fn flex_planned_dispatches(&mut self, device_id: &str) -> Result<Vec<PlannedDispatch>, Error> {
        let token = self.obtain_token().await?;
        let data: Result<FlexPlannedDispatchesData, Error> = self
            .post(
                KRAKEN_URL,
                "query FlexPlannedDispatches($deviceId: String!) {
                    flexPlannedDispatches(deviceId: $deviceId) {
                        start
                        end
                        type
                        energyAddedKwh
                    }
                }",
                json!({ "deviceId": device_id }),
                Some(&token),
            )
            .await;
        let mut dispatches = or_empty_without_device(data)?.flex_planned_dispatches.unwrap_or_default();
        sort_by_start(&mut dispatches, |d| &d.start)?;
        Ok(dispatches)
    }

    async fn post<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &str,
        variables: Value,
        token: Option<&str>,
    ) -> Result<T, Error> {
        let result = self.send(url, query, &variables, token).await;
        let Err(e) = &result else { return result };
        let Some(errors) = e.downcast_ref::<GraphQLErrors>().filter(|g| g.has_code(RATE_LIMITED)) else {
            return result;
        };

        let wait = match token {
            Some(token) => {
                let info: RateLimitInfoData = self.send(KRAKEN_URL, RATE_LIMIT_INFO_QUERY, &json!({}), Some(token)).await?;
                rate_limit_wait(&info.rate_limit_info, &errors.fields(), unix_now())?
            }
            None => TOKEN_RATE_LIMIT_WAIT,
        };
        eprintln!("Kraken rate limited {}; retrying in {}s", errors.fields().join(", "), wait.as_secs());
        tokio::time::sleep(wait).await;
        self.send(url, query, &variables, token).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &str,
        variables: &Value,
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
        parse_response(url, status, &body)
    }
}

/// Errors are checked before `data` is typed: alongside errors, `data` may be
/// null or partial in ways the typed structs reject.
fn parse_response<T: DeserializeOwned>(url: &str, status: reqwest::StatusCode, body: &str) -> Result<T, Error> {
    let parsed: GraphQLResponse<Value> = serde_json::from_str(body).map_err(|e| {
        let snippet: String = body.chars().take(300).collect();
        eyre!("{url} returned {status}, not GraphQL JSON ({e}): {snippet}")
    })?;
    serde_json::from_value(parsed.into_result()?)
        .map_err(|e| eyre!("{url}: unexpected GraphQL data shape ({e})"))
}

#[derive(Deserialize, Debug)]
struct GraphQLResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQLError>,
}

#[derive(Deserialize, Debug)]
pub struct GraphQLError {
    pub message: String,
    pub extensions: Option<GraphQLErrorExtensions>,
    /// Field names and list indexes down to the failing field.
    #[serde(default)]
    pub path: Vec<Value>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GraphQLErrorExtensions {
    pub error_code: Option<String>,
    pub error_type: Option<String>,
}

/// Kept typed inside the eyre report so callers can match on `errorCode`.
#[derive(Debug)]
pub struct GraphQLErrors(pub Vec<GraphQLError>);

impl GraphQLErrors {
    pub fn has_code(&self, code: &str) -> bool {
        self.0.iter().any(|e| {
            e.extensions.as_ref().and_then(|x| x.error_code.as_deref()) == Some(code)
        })
    }

    pub fn is_server_error(&self) -> bool {
        self.0.iter().any(|e| {
            e.extensions.as_ref().and_then(|x| x.error_type.as_deref()) == Some(SERVER_ERROR)
        })
    }

    /// The failing field of each error: the last name in its path.
    fn fields(&self) -> Vec<&str> {
        self.0.iter()
            .filter_map(|e| e.path.iter().rev().find_map(Value::as_str))
            .collect()
    }
}

impl fmt::Display for GraphQLErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let messages: Vec<&str> = self.0.iter().map(|e| e.message.as_str()).collect();
        write!(f, "graphql: {}", messages.join("; "))
    }
}

impl std::error::Error for GraphQLErrors {}

impl<T> GraphQLResponse<T> {
    fn into_result(self) -> Result<T, Error> {
        if !self.errors.is_empty() {
            return Err(GraphQLErrors(self.errors).into());
        }
        self.data.ok_or_eyre("graphql: response has neither data nor errors")
    }
}

const RATE_LIMIT_INFO_QUERY: &str = "query RateLimitInfo {
    rateLimitInfo {
        pointsAllowanceRateLimit { ttl isBlocked }
        fieldSpecificRateLimits(first: 100) {
            edges { node { field rate ttl isBlocked } }
        }
    }
}";

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RateLimitInfoData {
    rate_limit_info: RateLimitInfo,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RateLimitInfo {
    points_allowance_rate_limit: Option<RateLimitWindow>,
    field_specific_rate_limits: FieldRateLimits,
}

#[derive(Deserialize, Debug)]
struct FieldRateLimits {
    edges: Vec<FieldRateLimitEdge>,
}

#[derive(Deserialize, Debug)]
struct FieldRateLimitEdge {
    node: FieldRateLimit,
}

/// `field` is `Type.name` (e.g. `Query.flexPlannedDispatches`); `rate` is
/// e.g. `5/m`. A field can have several limits.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct FieldRateLimit {
    field: String,
    rate: String,
    #[serde(flatten)]
    window: RateLimitWindow,
}

/// `ttl` is the UNIX time the current window ends, blocked or not.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    ttl: i64,
    is_blocked: bool,
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Until the latest window blocking any of `fields` (or the points allowance)
/// ends. Zero when nothing is blocked any more.
fn rate_limit_wait(info: &RateLimitInfo, fields: &[&str], now: i64) -> Result<Duration, Error> {
    let blocked: Vec<(&str, &RateLimitWindow)> = info.field_specific_rate_limits.edges.iter()
        .map(|e| &e.node)
        .filter(|l| l.window.is_blocked)
        .filter(|l| fields.iter().any(|f| l.field.rsplit('.').next() == Some(*f)))
        .map(|l| (l.rate.as_str(), &l.window))
        .chain(info.points_allowance_rate_limit.iter()
            .filter(|w| w.is_blocked)
            .map(|w| ("points allowance", w)))
        .collect();

    let Some((rate, window)) = blocked.into_iter().max_by_key(|(_, w)| w.ttl) else {
        return Ok(Duration::ZERO);
    };
    // A second past the window end, in case the clocks disagree.
    let wait = Duration::from_secs((window.ttl - now + 1).max(0) as u64);
    if wait > MAX_RATE_LIMIT_WAIT {
        return Err(eyre!("{} rate limited ({rate}) until UNIX time {}", fields.join(", "), window.ttl));
    }
    Ok(wait)
}

fn or_empty_without_device<T: Default>(result: Result<T, Error>) -> Result<T, Error> {
    match result {
        Err(e) if e.downcast_ref::<GraphQLErrors>().is_some_and(|g| g.has_code(NO_DEVICE)) => {
            Ok(T::default())
        }
        other => other,
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

#[derive(Deserialize, Debug)]
struct AgreementsData {
    account: AgreementsAccount,
}

#[derive(Deserialize, Debug)]
struct AgreementsAccount {
    properties: Vec<AgreementsProperty>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AgreementsProperty {
    electricity_meter_points: Vec<MeterPointAgreements>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MeterPointAgreements {
    pub mpan: String,
    pub agreements: Vec<KrakenAgreement>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KrakenAgreement {
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub time_of_use_scheme: Option<TimeOfUseScheme>,
    pub tariff: KrakenTariff,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimeOfUseScheme {
    pub name: String,
    pub timezone: String,
    pub timeslots: Vec<TimeOfUseTimeslot>,
}

/// The day flags are all false when a timeslot applies every day.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TimeOfUseTimeslot {
    pub timeslot: String,
    pub active_from: String,
    pub active_to: String,
    pub weekdays: bool,
    pub weekends: bool,
    pub saturdays: bool,
    pub sundays: bool,
    pub workdays: bool,
    pub holidays: bool,
}

impl TimeOfUseTimeslot {
    pub fn is_every_day(&self) -> bool {
        ![self.weekdays, self.weekends, self.saturdays, self.sundays, self.workdays, self.holidays]
            .contains(&true)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KrakenTariff {
    pub tariff_code: String,
    pub product_code: String,
}

#[derive(Deserialize, Debug)]
struct DevicesData {
    devices: Option<Vec<Device>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: Option<String>,
    pub device_type: Option<String>,
    pub provider: Option<String>,
    pub status: Option<Value>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct CompletedDispatchesData {
    completed_dispatches: Option<Vec<Dispatch>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct PlannedDispatchesData {
    planned_dispatches: Option<Vec<Dispatch>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Dispatch {
    pub start: String,
    pub end: String,
    /// kWh, negative while charging. Seen as both a string and a number.
    #[serde(default, deserialize_with = "opt_f64")]
    pub delta: Option<f64>,
    #[serde(default)]
    pub meta: Option<DispatchMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DispatchMeta {
    pub location: Option<String>,
    /// `smart-charge`, `bump-charge`, ...
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct FlexPlannedDispatchesData {
    flex_planned_dispatches: Option<Vec<PlannedDispatch>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDispatch {
    pub start: String,
    pub end: String,
    pub r#type: Option<String>,
    #[serde(default, deserialize_with = "opt_f64")]
    pub energy_added_kwh: Option<f64>,
}

/// Kraken has no ordering argument, and offsets vary, so compare parsed instants.
fn sort_by_start<T>(items: &mut [T], start: impl Fn(&T) -> &str) -> Result<(), Error> {
    for item in items.iter() {
        str2dt(start(item))?;
    }
    items.sort_by_cached_key(|item| str2dt(start(item)).ok());
    Ok(())
}

fn opt_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(n.as_f64()),
        Value::String(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!("unexpected number: {other}"))),
    }
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
    fn agreements_response_deserializes() -> Result<(), Error> {
        let body = r#"{
            "data": {"account": {"properties": [{"electricityMeterPoints": [{
                "mpan": "1",
                "agreements": [
                    {"validFrom": "2025-12-27T00:00:00+00:00", "validTo": "2026-11-22T00:00:00+00:00",
                     "timeOfUseScheme": null,
                     "tariff": {"__typename": "HalfHourlyTariff",
                                "productCode": "AGILE-24-10-01", "tariffCode": "E-1R-AGILE-24-10-01-A"}},
                    {"validFrom": "2026-11-22T00:00:00+00:00", "validTo": null,
                     "timeOfUseScheme": {"name": "IOG_ECO7_TOU", "timezone": "Europe/London", "timeslots": [
                        {"timeslot": "ECO7_DAY", "activeFrom": "05:30:00", "activeTo": "23:30:00",
                         "weekdays": false, "weekends": false, "saturdays": false, "sundays": false,
                         "workdays": false, "holidays": false},
                        {"timeslot": "ECO7_NIGHT", "activeFrom": "23:30:00", "activeTo": "05:30:00",
                         "weekdays": false, "weekends": false, "saturdays": false, "sundays": false,
                         "workdays": false, "holidays": false}
                     ]},
                     "tariff": {"__typename": "HalfHourlyTariff",
                                "productCode": "IOG-SMB-VAR-26-10-29", "tariffCode": "E-1R-IOG-SMB-VAR-26-10-29-A"}}
                ]
            }]}]}}
        }"#;
        let data: GraphQLResponse<AgreementsData> = serde_json::from_str(body)?;
        let points = data.into_result()?.account.properties.remove(0).electricity_meter_points;
        let agreements = &points[0].agreements;
        assert_eq!(agreements[1].tariff.product_code, "IOG-SMB-VAR-26-10-29");
        assert_eq!(agreements[1].valid_to, None);
        assert!(agreements[0].time_of_use_scheme.is_none());
        let scheme = agreements[1].time_of_use_scheme.as_ref().ok_or_eyre("scheme")?;
        assert_eq!(scheme.timeslots[1].active_from, "23:30:00");
        assert!(scheme.timeslots.iter().all(TimeOfUseTimeslot::is_every_day));
        Ok(())
    }

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

    #[test]
    fn completed_dispatches_deserialize() -> Result<(), Error> {
        let body = r#"{
            "data": {
                "completedDispatches": [
                    {"start": "2026-11-22T14:00:00+00:00", "end": "2026-11-22T15:00:00+00:00",
                     "delta": "-3.5", "meta": {"location": "AT_HOME", "source": "smart-charge"}},
                    {"start": "2026-11-22T16:00:00+00:00", "end": "2026-11-22T16:30:00+00:00",
                     "delta": -1.25, "meta": {"location": null, "source": "bump-charge"}},
                    {"start": "2026-11-22T17:00:00+00:00", "end": "2026-11-22T17:30:00+00:00"}
                ]
            }
        }"#;

        let response: GraphQLResponse<CompletedDispatchesData> = serde_json::from_str(body)?;
        let dispatches = response.into_result()?.completed_dispatches.unwrap();

        assert_eq!(dispatches[0].delta, Some(-3.5), "string delta is parsed");
        assert_eq!(dispatches[1].delta, Some(-1.25), "numeric delta passes through");
        assert_eq!(dispatches[1].meta.as_ref().unwrap().source.as_deref(), Some("bump-charge"));
        assert!(dispatches[2].delta.is_none() && dispatches[2].meta.is_none(), "missing fields are tolerated");
        Ok(())
    }

    #[test]
    fn planned_dispatches_deserialize() -> Result<(), Error> {
        // First schedule after pairing the Ohme, 2026-11-22.
        let body = r#"{
            "data": {
                "plannedDispatches": [
                    {"delta": "-3.49", "end": "2026-11-22T22:30:00+00:00",
                     "meta": {"location": null, "source": "smart-charge"},
                     "start": "2026-11-22T22:00:00+00:00"},
                    {"delta": "-2.48", "end": "2026-11-23T12:00:00+00:00",
                     "meta": {"location": null, "source": "smart-charge"},
                     "start": "2026-11-23T11:30:00+00:00"}
                ]
            }
        }"#;

        let response: GraphQLResponse<PlannedDispatchesData> = serde_json::from_str(body)?;
        let dispatches = response.into_result()?.planned_dispatches.unwrap();
        assert_eq!(dispatches.len(), 2);
        assert_eq!(dispatches[0].delta, Some(-3.49));
        let meta = dispatches[0].meta.as_ref().unwrap();
        assert_eq!((meta.location.as_deref(), meta.source.as_deref()), (None, Some("smart-charge")));
        Ok(())
    }

    #[test]
    fn dispatches_sort_by_instant() -> Result<(), Error> {
        let body = r#"{
            "data": {
                "plannedDispatches": [
                    {"start": "2026-11-24T02:00:00+00:00", "end": "2026-11-24T03:00:00+00:00"},
                    {"start": "2026-11-24T01:00:00+01:00", "end": "2026-11-24T01:30:00+01:00"},
                    {"start": "2026-11-24T00:30:00+00:00", "end": "2026-11-24T01:00:00+00:00"}
                ]
            }
        }"#;

        let response: GraphQLResponse<PlannedDispatchesData> = serde_json::from_str(body)?;
        let mut dispatches = response.into_result()?.planned_dispatches.unwrap();
        sort_by_start(&mut dispatches, |d| &d.start)?;

        let starts: Vec<_> = dispatches.iter().map(|d| d.start.as_str()).collect();
        assert_eq!(starts, [
            "2026-11-24T01:00:00+01:00",
            "2026-11-24T00:30:00+00:00",
            "2026-11-24T02:00:00+00:00",
        ], "ordered by instant, not by string");
        Ok(())
    }

    #[test]
    fn unparsable_dispatch_start_fails_sort() {
        let mut starts = [String::from("2026-11-24T00:30:00+00:00"), String::from("soon")];
        assert!(sort_by_start(&mut starts, |s| s).is_err());
    }

    #[test]
    fn missing_device_means_no_dispatches() -> Result<(), Error> {
        let body = r#"{
            "data": {"completedDispatches": null},
            "errors": [{"message": "Unable to find device for given account.",
                        "extensions": {"errorType": "NOT_FOUND", "errorCode": "KT-CT-4301"}}]
        }"#;

        let response: GraphQLResponse<CompletedDispatchesData> = serde_json::from_str(body)?;
        let data = or_empty_without_device(response.into_result())?;
        assert!(data.completed_dispatches.unwrap_or_default().is_empty());
        Ok(())
    }

    #[test]
    fn other_errors_still_fail_dispatches() {
        let body = r#"{
            "data": {"completedDispatches": null},
            "errors": [{"message": "Unauthorized.", "extensions": {"errorCode": "KT-CT-1111"}}]
        }"#;

        let response: GraphQLResponse<CompletedDispatchesData> = serde_json::from_str(body).unwrap();
        assert!(or_empty_without_device(response.into_result()).is_err());
    }

    // As returned on 2026-11-23.
    const DEVICE_STATUS_ERROR: &str = r#"{"errors":[{"message":"Device status could not be fetched.","locations":[{"line":7,"column":25}],"path":["devices",0,"status"],"extensions":{"errorType":"APPLICATION","errorCode":"KT-CT-4360","errorDescription":"An internal error occurred. Please try again later."}}],"data":{"devices":null}}"#;

    #[test]
    fn device_status_failure_is_a_server_error() {
        let err = parse_response::<DevicesData>(KRAKEN_URL, reqwest::StatusCode::OK, DEVICE_STATUS_ERROR).unwrap_err();
        let errors = err.downcast_ref::<GraphQLErrors>().expect("typed GraphQL errors, not a shape error");
        assert!(errors.has_code("KT-CT-4360"));
        assert!(errors.is_server_error());
        assert_eq!(errors.fields(), ["status"]);
    }

    #[test]
    fn other_errors_are_not_server_errors() {
        let body = r#"{"data": null, "errors": [{"message": "Unauthorized.", "extensions": {"errorCode": "KT-CT-1111"}}]}"#;
        let err = parse_response::<DevicesData>(KRAKEN_URL, reqwest::StatusCode::OK, body).unwrap_err();
        assert!(!err.downcast_ref::<GraphQLErrors>().unwrap().is_server_error());
    }

    #[test]
    fn null_devices_without_errors_is_empty() -> Result<(), Error> {
        let data: DevicesData = parse_response(KRAKEN_URL, reqwest::StatusCode::OK, r#"{"data": {"devices": null}}"#)?;
        assert!(data.devices.unwrap_or_default().is_empty());
        Ok(())
    }

    #[test]
    fn mistyped_data_is_a_shape_error() {
        let err = parse_response::<DevicesData>(KRAKEN_URL, reqwest::StatusCode::OK, r#"{"data": {"devices": 1}}"#).unwrap_err();
        assert!(err.to_string().contains("unexpected GraphQL data shape"), "{err}");
    }

    // Shapes as returned on 2026-11-23 (see src/bin/debug_rate_limit.rs).
    const NOW: i64 = 1_795_450_570;

    fn rate_limit_info(flex_blocked: bool, points_blocked: bool) -> RateLimitInfo {
        let body = json!({
            "data": {"rateLimitInfo": {
                "pointsAllowanceRateLimit": {"ttl": 1_795_453_857, "isBlocked": points_blocked},
                "fieldSpecificRateLimits": {"edges": [
                    {"node": {"field": "Query.flexPlannedDispatches", "rate": "5/m", "ttl": 1_795_450_599, "isBlocked": flex_blocked}},
                    {"node": {"field": "Query.completedDispatches", "rate": "300/h", "ttl": 1_795_451_319, "isBlocked": true}},
                    {"node": {"field": "Mutation.obtainKrakenToken", "rate": "50/m", "ttl": 1_795_450_599, "isBlocked": false}},
                ]},
            }},
        });
        let response: GraphQLResponse<RateLimitInfoData> = serde_json::from_value(body).unwrap();
        response.into_result().unwrap().rate_limit_info
    }

    #[test]
    fn rate_limited_errors_name_their_field() -> Result<(), Error> {
        let body = r#"{
            "data": {"flexPlannedDispatches": null},
            "errors": [{
                "message": "Too many requests.",
                "path": ["flexPlannedDispatches"],
                "extensions": {"errorCode": "KT-CT-1199", "errorType": "AUTHORIZATION"}
            }, {
                "message": "Too many requests.",
                "path": ["account", "properties", 0, "electricityMeterPoints", 0, "agreements", 1, "timeOfUseScheme"],
                "extensions": {"errorCode": "KT-CT-1199"}
            }]
        }"#;

        let response: GraphQLResponse<FlexPlannedDispatchesData> = serde_json::from_str(body)?;
        let err = response.into_result().unwrap_err();
        let errors = err.downcast_ref::<GraphQLErrors>().unwrap();
        assert!(errors.has_code(RATE_LIMITED));
        assert_eq!(errors.fields(), ["flexPlannedDispatches", "timeOfUseScheme"]);
        Ok(())
    }

    #[test]
    fn waits_for_the_blocked_fields_window() -> Result<(), Error> {
        let wait = rate_limit_wait(&rate_limit_info(true, false), &["flexPlannedDispatches"], NOW)?;
        assert_eq!(wait, Duration::from_secs(30));
        Ok(())
    }

    #[test]
    fn retries_at_once_when_no_longer_blocked() -> Result<(), Error> {
        let wait = rate_limit_wait(&rate_limit_info(false, false), &["flexPlannedDispatches"], NOW)?;
        assert_eq!(wait, Duration::ZERO, "other blocked fields don't delay this one");
        Ok(())
    }

    #[test]
    fn long_blocks_fail_instead_of_waiting() {
        let err = rate_limit_wait(&rate_limit_info(false, false), &["completedDispatches"], NOW).unwrap_err();
        assert!(err.to_string().contains("300/h"), "{err}");

        let err = rate_limit_wait(&rate_limit_info(true, true), &["flexPlannedDispatches"], NOW).unwrap_err();
        assert!(err.to_string().contains("points allowance"), "{err}");
    }
}
