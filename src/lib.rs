//! # Unifi Access API Client
//!
//! This crate provides a client for the Unifi Access API based off of the documentation found here:
//!
//! <https://core-config-gfoz.uid.alpha.ui.com/configs/unifi-access/api_reference.pdf>
//!
//! This crate is a hand written wrapper of the described REST API, and is incomplete in coverage at the moment.
//! This crate was developed to support a Makerspace door access system and is being happily used in production for that application.
//!
//! Contributions to extend the functionality are welcome.
//!
//! To get started login to your Unifi Controller and go to:
//! Settings -> Security -> Advanced and create a new token. There is a link to the documentation for the API alongside the token.
//!
//! The API is only available on the LAN network of the controller, if you want to access the API from offsite you'll need to establish a VPN.
//!
//! A basic example:
//! ```no_run
//! use unifi_access::UnifiClient;
//! #[tokio::main]
//! async fn main() {
//!   let client = UnifiClient::new("192.168.1.1", "your_auth_token");
//!   let users = client.get_all_users().await.unwrap();
//!   println!("{users:?}");
//! }
//! ```
//!
//! Head to [UnifiClient] to see the available operations.
//!
//! The API is fully async and technically relies on `tokio`, but tokio could be removed if folks want a different runtime.

use std::sync::Mutex;

use log::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use simple_error::bail;
use ts_rs::TS;

/// The base client object that operations are provided on.
pub struct UnifiClient {
    client: reqwest::Client,
    auth_token: String,
    host: String,
}

/// Represents a user in the unifi system.
/// This is used with serde_json to serialize and deserialize the JSON responses from the API.
#[derive(Debug, Serialize, Deserialize, Clone, TS)]
pub struct User {
    /// ID is in the form of a uuid
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub nfc_cards: Vec<NfcCard>,
    pub employee_number: String,
    pub user_email: String,
    /// Doing a bit of a hack here
    /// access_policies isn't provided in the main users API by unifi
    /// But we need for our use case so we're including it here
    pub access_policies: Option<Vec<AccessPolicy>>,
}

/// Represents an NFC card in the unifi system.
#[derive(Debug, Serialize, Deserialize, Clone, TS)]
pub struct NfcCard {
    /// Display name of the card in UI
    pub id: String,
    /// Actual NFC token
    pub token: String,
}

/// The response format for a list of users
#[derive(Debug, Deserialize)]
pub struct UsersResponse {
    pub data: Vec<User>,
    // Additional unused fields: msg, code, pagination
}

/// This is the standard response format for all endpoints
// TODO make enum for code
#[derive(Debug, Deserialize)]
struct GenericResponse {
    pub data: Option<serde_json::Value>,
    pub msg: String,
    pub code: String,
}

/// Represents an access policy in the unifi system
#[derive(Debug, Deserialize, Serialize, Clone, TS)]
pub struct AccessPolicy {
    // UUID of the policy
    pub id: String,
    pub name: String,
    // Ignoring this for now as I don't need it
    // pub resources: Vec<String>,
    // type
    // schedule_id
}

/// Represents a physical device within the building
#[derive(Debug, Deserialize)]
pub struct Device {
    // Oddly device ids are not uuids...🤷
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
}

/// The available system log topics within unifi
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SystemLogTopic {
    All,
    DoorOpenings,
    Critical,
    Updates,
    DeviceEvents,
    AdminActivity,
    Visitor,
}

/// An individual entry in the unifi system log
// TODO there is a ton of data available in here only parsing out minimal for now
#[derive(Debug, Deserialize)]
pub struct SystemLogEvent {
    pub actor: serde_json::Value,
    pub authentication: serde_json::Value,
    pub event: serde_json::Value,
    pub target: serde_json::Value,
    // tag: String,
}

/// Weirdly nested structure returned by the system log endpoint
#[derive(Debug, Deserialize)]
pub struct SystemLogEventWrapper {
    #[serde(rename = "@timestamp")]
    pub timestamp: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_source")]
    pub source: SystemLogEvent,
}

/// Full response from system log endpoint
// TODO actual responses we're getting have different format than linked manual
// looks like this API is under some flux...
#[derive(Debug, Deserialize)]
pub struct SystemLogResponse {
    hits: Vec<SystemLogEventWrapper>,
    // pages: u32,
    // total: u32,
}

/// The error type for this crate
type UnifiError = Box<dyn std::error::Error + Send + Sync>;

/// The result type for this crate
type UnifiResult<T> = Result<T, UnifiError>;

impl UnifiClient {
    /// Creates a new client against the given address with the given auth token
    /// You can create an auth token in the Unifi Access UI by going to:
    /// Applications -> Access -> Settings -> Security -> Advanced
    /// Unifi Access's API is only available on the LAN network of the controller.
    /// The default port for Unifi Access is 12445.
    /// Unifi Access can only be reached over https
    ///
    /// For full documentation of the API see:
    ///
    /// <https://core-config-gfoz.uid.alpha.ui.com/configs/unifi-access/api_reference.pdf>
    pub fn new(hostname: &str, key: &str) -> UnifiClient {
        let client = reqwest::Client::builder()
            // The SSL cert is self-signed and untrusted
            // We have to disable cert checking to get around this
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        UnifiClient {
            client,
            auth_token: key.to_string(),
            host: hostname.to_string(),
        }
    }

    /// Internal function that wraps all requests
    async fn generic_request_raw(
        &self,
        method: reqwest::Method,
        api_path: String,
        body: Option<serde_json::Value>,
    ) -> UnifiResult<String> {
        let url = format!("https://{}:12445{}", self.host, api_path);
        debug!("Sending request: {method} {url} {body:?}");
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.auth_token);
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let response = request.send().await?.text().await?;
        trace!("Got raw response: {response}");
        Ok(response)
    }

    /// Generically hits an endpoint and handles the response code without deserializing the "data" field
    async fn generic_request_no_parse(
        &self,
        method: reqwest::Method,
        api_path: String,
        body: Option<serde_json::Value>,
    ) -> UnifiResult<Option<serde_json::Value>> {
        let response = self
            .generic_request_raw(method, api_path.clone(), body)
            .await?;
        trace!("Got response from unifi: {response}");
        let parsed: GenericResponse = serde_json::from_str(&response)?;
        if parsed.code != "SUCCESS" {
            bail!("Failed request to {api_path}: {}", parsed.msg);
        }
        Ok(parsed.data)
    }

    /// Generically hits and endpoint, handles the response code, and tries to deserialize the "data" field
    async fn generic_request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        api_path: String,
        body: Option<serde_json::Value>,
    ) -> UnifiResult<T> {
        let raw = self
            .generic_request_no_parse(method, api_path.clone(), body)
            .await?;
        Ok(serde_json::from_value(raw.ok_or(
            simple_error::SimpleError::new(format!("No data found in response")),
        )?)?)
    }

    /// Gets a list of all users.
    /// Endpoint supports partial fetches and pagination, not using those yet.
    /// Endpoint supports optionally getting access policy info, not implementing that yet.
    pub async fn get_all_users(&self) -> UnifiResult<Vec<User>> {
        self.generic_request(
            reqwest::Method::GET,
            "/api/v1/developer/users".to_string(),
            None,
        )
        .await
    }

    /// The same as get_all_users but also collects the access policies for each user.
    /// Does so by making an additional request for each user, can be slow for large numbers of users.
    pub async fn get_all_users_with_access_information(&self) -> UnifiResult<Vec<User>> {
        let mut users = self.get_all_users().await?;
        for user in users.iter_mut() {
            user.access_policies = Some(self.get_access_policies_for_user(&user.id).await?);
        }
        Ok(users)
    }

    /// Registers a new user
    /// Returns the UUID of the newly created user if registration was successful
    pub async fn register_user(
        &self,
        first_name: String,
        last_name: String,
        email: String,
        employee_number: String,
    ) -> UnifiResult<String> {
        debug!("Sending register_user_request: {first_name} {last_name} {email} {employee_number}");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
        let register_user_response: serde_json::Value = self
            .generic_request(
                reqwest::Method::POST,
                "/api/v1/developer/users".to_string(),
                Some(json!({
                    "first_name": first_name,
                    "last_name": last_name,
                    "user_email": email,
                    "employee_number": employee_number,
                    "onboard_time": now.as_secs(),
                })),
            )
            .await?;
        let id = register_user_response
            .get("id")
            .ok_or(simple_error::SimpleError::new("id not found in response"))?
            .as_str()
            .ok_or(simple_error::SimpleError::new("id not a string"))?;
        Ok(id.to_string())
    }

    /// Retrieves the list of access policies
    pub async fn get_all_access_policies(&self) -> UnifiResult<Vec<AccessPolicy>> {
        debug!("Sending get_all_access_policies_request");
        self.generic_request(
            reqwest::Method::GET,
            "/api/v1/developer/access_policies".to_string(),
            None,
        )
        .await
    }

    /// Returns the details of an individual user by their uuid
    pub async fn get_user_by_id(&self, user_id: &str) -> UnifiResult<User> {
        debug!("Sending get_user_by_id_request: {user_id}");
        self.generic_request(
            reqwest::Method::GET,
            format!("/api/v1/developer/users/{}", user_id),
            None,
        )
        .await
    }

    /// Assigns an access policy to a user
    pub async fn assign_access_policies(
        &self,
        user_id: &str,
        policy_ids: Vec<String>,
    ) -> UnifiResult<()> {
        let api = format!("/api/v1/developer/users/{}/access_policies", user_id);
        debug!("Sending assign_access_policy_request: {user_id} {policy_ids:?} to {api}");
        let _ = self
            .generic_request_no_parse(
                reqwest::Method::PUT,
                api,
                Some(json!({
                    "access_policy_ids": policy_ids,
                })),
            )
            .await?;
        Ok(())
    }

    /// Removes all access policies from a user making them effectively inactive, but retaining the NFC card information
    pub async fn remove_all_access_policies_from_user(&self, user_id: &str) -> UnifiResult<()> {
        let api = format!("/api/v1/developer/users/{}/access_policies", user_id);
        debug!("Sending assign_access_policy_request to remove access: {user_id} to {api}");
        let _ = self
            .generic_request_no_parse(
                reqwest::Method::PUT,
                api,
                Some(json!({
                    "access_policy_ids": [],
                })),
            )
            .await?;
        Ok(())
    }

    /// Retrieves the list of access policies for a given user
    pub async fn get_access_policies_for_user(
        &self,
        user_id: &str,
    ) -> UnifiResult<Vec<AccessPolicy>> {
        let api = format!("/api/v1/developer/users/{}/access_policies", user_id);
        debug!("Sending get_access_policies_for_user_request: {user_id} to {api}");
        let response = self
            .generic_request(reqwest::Method::GET, api, None)
            .await?;
        Ok(response)
    }

    /// Retrieves a list of all devices
    pub async fn get_devices(&self) -> UnifiResult<Vec<Device>> {
        // Weirdly this endpoint returns a list of lists of devices for no reason
        let response: Vec<Vec<Device>> = self
            .generic_request(
                reqwest::Method::GET,
                "/api/v1/developer/devices".to_string(),
                None,
            )
            .await?;
        Ok(response.into_iter().flatten().collect())
    }

    /// Starts a session on a specific reader device to enroll a new card
    /// Returns the created session id if successful
    /// The reader will now poll for a card
    pub async fn start_nfc_enrollment_session(&self, device_id: &str) -> UnifiResult<String> {
        let enroll_response: serde_json::Value = self
            .generic_request(
                reqwest::Method::POST,
                "/api/v1/developer/credentials/nfc_cards/sessions".to_string(),
                Some(json!({
                    "device_id": device_id,
                    // Setting this as default for now
                    "reset_ua_card": true
                })),
            )
            .await?;
        let session_id = enroll_response
            .get("session_id")
            .ok_or(simple_error::SimpleError::new(
                "session_id not found in response",
            ))?
            .as_str()
            .ok_or(simple_error::SimpleError::new("session_id not a string"))?;
        Ok(session_id.to_string())
    }

    /// Hits the session status endpoint a single time
    /// If there is an error reading the session returns an error
    /// If the session is found, but a card not issued yet, returns None
    /// Otherwise returns the scanned in card
    pub async fn get_nfc_enrollment_session_status(
        &self,
        session_id: &str,
    ) -> UnifiResult<Option<NfcCard>> {
        let response = self
            .generic_request_raw(
                reqwest::Method::GET,
                format!(
                    "/api/v1/developer/credentials/nfc_cards/sessions/{}",
                    session_id
                ),
                None,
            )
            .await?;

        // Check if we got the "SESSION_NOT_FOUND" meaning it has been cancelled
        if response.to_string().contains("SESSION_NOT_FOUND") {
            return Err(Box::new(simple_error::SimpleError::new(
                "Session has been canceled",
            )));
        }
        if response.to_string().contains("TOKEN_EMPTY") {
            // We don't have a card yet
            return Ok(None);
        }
        // Parse as JSON, strip the code and parse body
        let parsed: GenericResponse = serde_json::from_str(&response)?;

        let body = parsed
            .data
            .ok_or(simple_error::SimpleError::new("data not found in response"))?;

        // Otherwise try to parse response as card and return it
        let x: Option<NfcCard> = serde_json::from_value(body)?;
        Ok(x)
    }

    /// Complete a single card enrollment on the device
    /// Will start an enrollment session, and poll until the card is scanned
    pub async fn enroll_nfc_card(
        &self,
        device_id: &str,
        session_state: &Mutex<Option<String>>,
    ) -> UnifiResult<NfcCard> {
        let session = self.start_nfc_enrollment_session(device_id).await?;
        *session_state.lock().unwrap() = Some(session.clone());
        loop {
            let result = self.get_nfc_enrollment_session_status(&session).await;
            match result {
                Ok(Some(card)) => return Ok(card),
                Ok(None) => {
                    // Wait and read again
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    /// Assigns a card to a user
    pub async fn assign_nfc_card(&self, user_id: &str, card: &NfcCard) -> UnifiResult<()> {
        self.generic_request_no_parse(
            reqwest::Method::PUT,
            format!("/api/v1/developer/users/{}/nfc_cards", user_id),
            Some(json!({
                "token": card.token,
            })),
        )
        .await?;
        Ok(())
    }

    /// Fetches the user id of the user the card is assigned to if any
    pub async fn fetch_nfc_card_user(&self, card: &NfcCard) -> UnifiResult<Option<String>> {
        // We get a lot more data from the response, but this is all we need to parse
        #[derive(Debug, Deserialize)]
        struct CardUser {
            user_id: Option<String>,
        }
        let x: CardUser = self
            .generic_request(
                reqwest::Method::GET,
                format!(
                    "/api/v1/developer/credentials/nfc_cards/tokens/{}",
                    card.token
                ),
                None,
            )
            .await?;
        Ok(x.user_id)
    }

    /// Removes an NFC card from the system entirely
    /// This will find any users the card is enrolled to and unassign the card from them
    /// Card will need to be re-enrolled to be used again
    pub async fn remove_nfc_card(&self, card: &NfcCard) -> UnifiResult<()> {
        // Fetch the card data to see if it assigned to anyone
        let user = self.fetch_nfc_card_user(card).await?;
        if let Some(user_id) = user {
            info!("Unassigning card {card:?} from user {user_id}");
            // Unassign the card from the user
            self.generic_request_no_parse(
                reqwest::Method::PUT,
                format!("/api/v1/developer/users/{}/nfc_cards/delete", user_id),
                Some(json!({
                    "token": card.token,
                })),
            )
            .await?;
        }

        // Actually delete the card
        info!("Deleting card {card:?}");
        let endpoint = format!(
            "/api/v1/developer/credentials/nfc_cards/tokens/{}",
            card.token
        );
        self.generic_request_no_parse(reqwest::Method::DELETE, endpoint, None)
            .await?;
        info!("Card deleted successfully");
        Ok(())
    }

    /// Ends an ongoing enrollment session
    pub async fn end_enrollment_session(&self, session_id: &str) -> UnifiResult<()> {
        self.generic_request_no_parse(
            reqwest::Method::DELETE,
            format!(
                "/api/v1/developer/credentials/nfc_cards/sessions/{}",
                session_id
            ),
            None,
        )
        .await?;
        Ok(())
    }

    /// Accesses the system log for the device. The system log contains a variety of useful
    /// information about the system, but can be overwhelming and requires pagination.
    // TODO optional parameters: pagination, start and end times,
    // TODO this function likely not recommended for use until we get it cleaned up more
    pub async fn fetch_system_log(
        &self,
        topic: SystemLogTopic,
        start_time: Option<std::time::SystemTime>,
    ) -> UnifiResult<Vec<SystemLogEventWrapper>> {
        let body = json!({
            "topic": topic,
            "since": start_time.map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
        });
        let full_response: SystemLogResponse = self
            .generic_request(
                reqwest::Method::POST, // Unifi... why is this a post?
                "/api/v1/developer/system/logs".to_string(),
                Some(body),
            )
            .await?;
        Ok(full_response.hits)
    }
}
