use eyre::{Error, OptionExt};
use chrono::Utc;
use serde::{Serialize, Deserialize};
use std::{
    fs::{self, File},
    path::Path,
    time::Duration,
};

use crate::{
    times::TimeRange,
};

// Hardcoded value, unknown origin
const OHME_GOOGLE_PUBLIC_KEY: &'static str = "AIzaSyC8ZeZngm33tpOXLpbXeKfwtyZ1WrkbdBY";

#[derive(Debug)]
pub struct OhmeClient {
    username: String,
    password: String,
    identity: Option<IdentityToolkitVerifyPasswordResponse>,
    token: Option<SecureTokenResponse>,
}

#[derive(Serialize, Debug)]
pub struct IdentityToolkitData {
    email: String,
    password: String,
    #[serde(rename = "returnSecureToken")]
    return_secure_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IdentityToolkitVerifyPasswordResponse {
    r#kind: String,
    #[serde(rename = "localId")]
    local_id: String,
    email: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "idToken")]
    id_token: String,
    registered: bool,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: String,
}

#[derive(Serialize, Debug)]
pub struct SecureTokenData {
    #[serde(rename = "grantType")]
    grant_type: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SecureTokenResponse {
    token_type: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    user_id: String,
    project_id: String,
    expires_in: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Account {
    cars: Vec<Car>,
    #[serde(rename = "chargeDevices")]
    charge_devices: Vec<ChargeDevice>,
    tariff: Tariff,
    user: User,
    #[serde(rename = "userCapabilities")]
    user_capabilities: UserCapabilities,
    #[serde(rename = "userSettings")]
    user_settings: UserSettings,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Car {
  id: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ChargeDevice {
    id: String,
    #[serde(rename = "modelType")]
    model_type: String,
    #[serde(rename = "modelTypeDisplayName")]
    model_type_display_name: String,
    #[serde(rename = "modelCapabilities")]
    model_capabilities: ModelCapabilities,
    #[serde(rename = "optionalSettings")]
    optional_settings: OptionalSettings,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ModelCapabilities {
    stealth: bool,
    #[serde(rename = "buttonsLockable")]
    buttons_lockable: bool,
    #[serde(rename = "pluginsRequireApprovalMode")]
    plugins_require_approval_mode: bool,
    #[serde(rename = "peakChargingAvoidable")]
    peak_charging_avoidable: bool,
    #[serde(rename = "userDefinedMaxAmpsConfigurable")]
    user_defined_max_amps_configurable: bool,
    #[serde(rename = "randomisedDelay")]
    randomised_delay: bool,
    #[serde(rename = "remotelyServiceable")]
    remotely_serviceable: bool,
    #[serde(rename = "advancedSettingsReadable")]
    advanced_settings_readable: bool,
    #[serde(rename = "solarModes")]
    solar_modes: Vec<String>,
    #[serde(rename = "defaultSolarMode")]
    default_solar_mode: String,
    #[serde(rename = "wifiSupport")]
    wifi_support: bool,
    #[serde(rename = "wifiType")]
    wifi_type: String,
    #[serde(rename = "multiPhaseSupport")]
    multi_phase_support: bool,
    #[serde(rename = "penFaultCompliance")]
    pen_fault_compliance: bool,
    #[serde(rename = "areraSupport")]
    arera_support: bool,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct OptionalSettings {
    #[serde(rename = "buttonsLocked")]
    buttons_locked: bool,
    #[serde(rename = "stealthEnabled")]
    stealth_enabled: bool,
    #[serde(rename = "pluginsRequireApproval")]
    plugins_require_approval: bool,
    #[serde(rename = "avoidPeakCharging")]
    avoid_peak_charging: bool,
    #[serde(rename = "randomisedDelaySeconds")]
    randomised_delay_seconds: u16,
    #[serde(rename = "allowInstallerMode")]
    allow_installer_mode: bool,
    #[serde(rename = "solarMode")]
    solar_mode: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct Tariff {
    id: String,
    #[serde(rename = "supplierId")]
    supplier_id: String,
    #[serde(rename = "supplierDisplayName")]
    supplier_display_name: String,
    #[serde(rename = "tariffDisplayName")]
    tariff_display_name: String,
    #[serde(rename = "tariffFullName")]
    tariff_full_name: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct User {
    id: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct UserCapabilities {
    #[serde(rename = "usageExport")]
    usage_export: bool,
    #[serde(rename = "userDefinedMaxAmps")]
    user_defined_max_amps: bool,
    #[serde(rename = "multipleCars")]
    multiple_cars: bool,
    #[serde(rename = "manualSoc")]
    manual_soc: bool,
    #[serde(rename = "offPeakQuotaConfig")]
    off_peak_quota_config: bool,
    #[serde(rename = "aiChatBot")]
    ai_chat_bot: bool,
    #[serde(rename = "noChargeQuotaEnforcement")]
    no_charge_quota_enforcement: bool,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct UserSettings {
    #[serde(rename = "pushNotificationsEnabled")]
    push_notifications_enabled: bool,
    #[serde(rename = "unitSystem")]
    unit_system: String,
}

#[derive(Deserialize, Debug)]
pub struct ChargeSummary {
    pub granularity: String,
    pub stats: Vec<ChargeSummaryStatistics>,
    //#[serde(rename = "totalStats")]
    //total_stats: ChargeSummaryStats,
}

#[derive(Deserialize, Debug)]
pub struct ChargeSummaryStatistics {
    #[serde(rename = "startTime")]
    pub start_time: i64,
    #[serde(rename = "endTime")]
    pub end_time: i64,
    #[serde(rename = "energyChargedTotalWh")]
    pub energy_charged_total_wh: i32,
    #[serde(rename = "solarEnergyChargedWh")]
    pub solar_energy_charged_wh: i32,
}

impl OhmeClient {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            identity: None,
            token: None,
        }
    }

    pub fn authenticate(mut self, username: &str, password: &str) -> Self {
        self.username = String::from(username);
        self.password = String::from(password);
        self
    }

    pub async fn verify_password(&mut self) -> Result<(), Error> {
        let payload =  IdentityToolkitData {
            email: self.username.clone(),
            password: self.password.clone(),
            return_secure_token: String::from("True"),
        };

        //eprintln!("{}", serde_urlencoded::to_string(&payload)?);

        let url = "https://www.googleapis.com/identitytoolkit/v3/relyingparty/verifyPassword";
        let client = reqwest::Client::new();
        let request = client
            .post(url)
            .timeout(Duration::from_secs(15))
            .query(&[
                ("key", String::from(OHME_GOOGLE_PUBLIC_KEY)),
            ])
            .form(&payload)
            .build()?;
        //eprintln!("{:#?}", request);

        let response = client
            .execute(request)
            .await?;

        //eprintln!("{:#?}", response);

        let identity: IdentityToolkitVerifyPasswordResponse = response.json().await?;
        self.identity = Some(identity);

        //let identity: serde_json::Value = response.json().await?;
        //eprintln!("{:#?}", identity);

        Ok(())
    }

    pub fn write_token<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        fs::write(path.as_ref(), serde_json::to_vec_pretty(&self.identity)?)?;
        Ok(())
    }

    pub fn read_token<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let f = File::open(path.as_ref())?;
        let identity: IdentityToolkitVerifyPasswordResponse = serde_json::from_reader(f)?;

        self.identity = Some(identity);
        Ok(())
    }

    pub async fn refresh_token(&mut self) -> Result<(), Error> {
        if let None = self.identity {
            self.verify_password().await?;
        }

        let mut refresh_token = self.identity
            .clone()
            .ok_or_eyre("no identity")?
            .refresh_token;

        if let Some(token) = &self.token {
            refresh_token = token.refresh_token.clone();
        }

        let url = "https://securetoken.googleapis.com/v1/token";
        let response = reqwest::Client::new()
            .post(url)
            .timeout(Duration::from_secs(5))
            .query(&[
                ("key", String::from(OHME_GOOGLE_PUBLIC_KEY)),
            ])
            .json(&SecureTokenData {
                grant_type: String::from("refresh_token"),
                refresh_token: String::from(refresh_token),
            })
            .send()
            .await?;

        let token: SecureTokenResponse = response.json().await?;
        self.token = Some(token);

        Ok(())
    }

    pub fn write_refresh_token<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        fs::write(path.as_ref(), serde_json::to_vec_pretty(&self.token)?)?;
        Ok(())
    }

    pub fn read_refresh_token<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let f = File::open(path.as_ref())?;
        let token: SecureTokenResponse = serde_json::from_reader(f)?;

        self.token = Some(token);
        Ok(())
    }


    pub async fn account_info(&self) -> Result<Account, Error> {
        let id_token = self.token
            .clone()
            .ok_or_eyre("no token")?
            .id_token;

        let url = "https://api.ohme.io/v1/users/me/account";
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .header("Authorization", format!("Firebase {}", id_token))
            .send()
            .await?;

        let account: Account = response.json().await?;
        Ok(account)
    }

    pub async fn get_charge_summary(&self, user_id: &str, timerange: Option<TimeRange>) -> Result<ChargeSummary, Error> {
        let id_token = self.token
            .clone()
            .ok_or_eyre("no token")?
            .id_token;

        //eprintln!("{timerange:#?}");

        let (start, end): (i64, i64) = if let Some(timerange) = timerange {
            (
                timerange.start.timestamp_millis() ,
                timerange.end.timestamp_millis(),
            )
        } else {
            let end = Utc::now().timestamp_millis();
            let start = end - (24 * 60 * 60 * 1000);
            (start, end)
        };

        let url = format!("https://api.ohme.io/v1/chargeSessions/summary/users/{user_id}");
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .header("Authorization", format!("Firebase {}", id_token))
            .query(&[
                ("startTs", start.to_string()),
                ("endTs", end.to_string()),
                ("granularity", String::from("HALF_HOUR")),
            ])
             .send()
            .await?;

        //eprintln!("{:#?}", response);

        //let data: serde_json::Value = response.json().await?;
        //eprintln!("{:#?}", data);

        let summary: ChargeSummary = response.json().await?;
        Ok(summary)
    }
}

