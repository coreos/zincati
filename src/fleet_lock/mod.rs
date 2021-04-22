//! Asynchronous FleetLock client, remote lock management.
//!
//! This module implements a client for FleetLock, a bare HTTP
//! protocol for managing cluster-wide reboot via a remote
//! lock manager. Protocol specification is available at
//! https://coreos.github.io/zincati/development/fleetlock/protocol/ .

use crate::identity::Identity;
use anyhow::{Context, Result};
use futures::prelude::*;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[cfg(test)]
mod mock_tests;

/// Default timeout for HTTP requests completion (30 minutes).
const DEFAULT_HTTP_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// FleetLock pre-reboot API path endpoint (v1).
static V1_PRE_REBOOT: &str = "v1/pre-reboot";

/// FleetLock steady-state API path endpoint (v1).
static V1_STEADY_STATE: &str = "v1/steady-state";

/// FleetLock JSON protocol: service error.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RemoteJsonError {
    /// Machine-friendly brief error kind.
    kind: String,
    /// Human-friendly detailed error explanation.
    value: String,
}

/// Error related to the FleetLock service.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum FleetLockError {
    /// Remote endpoint error.
    Remote(reqwest::StatusCode, RemoteJsonError),
    /// Generic HTTP error.
    Http(reqwest::StatusCode),
    /// Client builder failed.
    FailedClientBuilder(String),
    /// Client failed request.
    FailedRequest(String),
}

impl FleetLockError {
    /// Return the machine-friendly brief error kind.
    pub fn error_kind(&self) -> String {
        match *self {
            FleetLockError::Remote(_, ref err) => err.kind.clone(),
            FleetLockError::Http(status) => format!("generic_http_{}", status.as_u16()),
            FleetLockError::FailedClientBuilder(_) => "client_failed_build".to_string(),
            FleetLockError::FailedRequest(_) => "client_failed_request".to_string(),
        }
    }

    /// Return the human-friendly detailed error explanation.
    pub fn error_value(&self) -> String {
        match *self {
            FleetLockError::Remote(_, ref err) => err.value.clone(),
            FleetLockError::Http(_) => "(unknown/generic server error)".to_string(),
            FleetLockError::FailedClientBuilder(ref err)
            | FleetLockError::FailedRequest(ref err) => err.clone(),
        }
    }

    /// Return the server-side error status code, if any.
    pub fn status_code(&self) -> Option<u16> {
        match *self {
            FleetLockError::Remote(s, _) | FleetLockError::Http(s) => Some(s.as_u16()),
            _ => None,
        }
    }
}

impl std::fmt::Display for FleetLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Account for both server-side and client-side failures.
        let context = match self.status_code() {
            Some(s) => format!("server-side error, code {}", s),
            None => "client-side error".to_string(),
        };
        write!(f, "{}: {}", context, self.error_value())
    }
}

/// Client to make outgoing API requests.
#[derive(Clone, Debug, Serialize)]
pub struct Client {
    /// Base URL for API endpoint.
    #[serde(skip)]
    api_base: reqwest::Url,
    /// Asynchronous reqwest client.
    #[serde(skip)]
    hclient: reqwest::Client,
    /// Request body.
    body: String,
}

impl Client {
    /// Try to lock a semaphore slot on the remote manager.
    ///
    /// It returns `true` if the operation succeeds, or a `FleetLockError`
    /// with the relevant error explanation.
    pub fn pre_reboot(&self) -> impl Future<Output = Result<bool, FleetLockError>> {
        let req = self
            .new_request(Method::POST, V1_PRE_REBOOT)
            .map_err(|e| FleetLockError::FailedClientBuilder(e.to_string()));

        futures::future::ready(req)
            .and_then(|req| {
                req.send()
                    .map_err(|e| FleetLockError::FailedRequest(e.to_string()))
            })
            .and_then(Self::map_response)
    }

    /// Try to unlock a semaphore slot on the remote manager.
    ///
    /// It returns `true` if the operation succeeds, or a `FleetLockError`
    /// with the relevant error explanation.
    pub fn steady_state(&self) -> impl Future<Output = Result<bool, FleetLockError>> {
        let req = self
            .new_request(Method::POST, V1_STEADY_STATE)
            .map_err(|e| FleetLockError::FailedClientBuilder(e.to_string()));

        futures::future::ready(req)
            .and_then(|req| {
                req.send()
                    .map_err(|e| FleetLockError::FailedRequest(e.to_string()))
            })
            .and_then(Self::map_response)
    }

    /// Return a request builder for the target URL, with proper parameters set.
    fn new_request<S: AsRef<str>>(
        &self,
        method: reqwest::Method,
        url_suffix: S,
    ) -> Result<reqwest::RequestBuilder> {
        let url = self.api_base.clone().join(url_suffix.as_ref())?;
        let builder = self
            .hclient
            .request(method, url)
            .body(self.body.clone())
            .header("fleet-lock-protocol", "true");
        Ok(builder)
    }

    /// Map an HTTP response to a service result.
    async fn map_response(response: reqwest::Response) -> Result<bool, FleetLockError> {
        // On success, short-circuit to `true`.
        let status = response.status();
        if status.is_success() {
            return Ok(true);
        }

        // On error, decode failure details (or synthesize a generic error).
        match response.json::<RemoteJsonError>().await {
            Ok(rej) => Err(FleetLockError::Remote(status, rej)),
            _ => Err(FleetLockError::Http(status)),
        }
    }
}

/// Client builder.
#[derive(Clone, Debug)]
pub struct ClientBuilder {
    /// Base URL for API endpoint (mandatory).
    api_base: String,
    /// Asynchronous reqwest client (custom).
    hclient: Option<reqwest::Client>,
    /// Client identity.
    client_identity: ClientIdentity,
}

/// Client identity, for requests body.
#[derive(Clone, Debug, Serialize)]
pub struct ClientIdentity {
    client_params: ClientParameters,
}

/// Client parameters.
#[derive(Clone, Debug, Serialize)]
pub struct ClientParameters {
    /// Node identifier, for lock ownership.
    id: String,
    /// Reboot group, for role-specific remote configuration.
    group: String,
}

impl ClientBuilder {
    /// Return a new client builder for the given base API endpoint URL.
    pub(crate) fn new<T>(api_base: T, identity: &Identity) -> Self
    where
        T: Into<String>,
    {
        Self {
            api_base: api_base.into(),
            hclient: None,
            client_identity: ClientIdentity {
                client_params: ClientParameters {
                    id: identity.node_uuid.lower_hex(),
                    group: identity.group.clone(),
                },
            },
        }
    }

    /// Set (or reset) the HTTP client to use.
    #[allow(dead_code)]
    pub fn http_client(self, hclient: Option<reqwest::Client>) -> Self {
        let mut builder = self;
        builder.hclient = hclient;
        builder
    }

    /// Build a client with specified parameters.
    pub fn build(self) -> Result<Client> {
        let hclient = match self.hclient {
            Some(client) => client,
            None => reqwest::ClientBuilder::new()
                .timeout(DEFAULT_HTTP_COMPLETION_TIMEOUT)
                .build()?,
        };

        let api_base = reqwest::Url::parse(&self.api_base)
            .context(format!("failed to parse '{}'", &self.api_base))?;
        if self.client_identity.client_params.group.is_empty() {
            anyhow::bail!("missing group value");
        }
        let body = serde_json::to_string_pretty(&self.client_identity)?;
        let client = Client {
            api_base,
            hclient,
            body,
        };
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::response::Response;
    use http::status::StatusCode;
    use tokio::runtime as rt;

    #[test]
    fn test_service_rejection_display() {
        let err_body = r#"
{
  "kind": "failure_foo",
  "value": "failed to perform foo"
}
"#;
        let runtime = rt::Runtime::new().unwrap();
        let response = Response::builder().status(466).body(err_body).unwrap();
        let fut_rejection = Client::map_response(response.into());
        let rejection = runtime.block_on(fut_rejection).unwrap_err();
        let expected_rejection = FleetLockError::Remote(
            StatusCode::from_u16(466).unwrap(),
            RemoteJsonError {
                kind: "failure_foo".to_string(),
                value: "failed to perform foo".to_string(),
            },
        );
        assert_eq!(&rejection, &expected_rejection);

        let msg = rejection.to_string();
        let expected_msg = "server-side error, code 466: failed to perform foo";
        assert_eq!(&msg, expected_msg);
    }

    #[test]
    fn test_http_error_display() {
        let runtime = rt::Runtime::new().unwrap();
        let response = Response::builder().status(433).body("").unwrap();
        let fut_rejection = Client::map_response(response.into());
        let rejection = runtime.block_on(fut_rejection).unwrap_err();
        let expected_rejection = FleetLockError::Http(StatusCode::from_u16(433).unwrap());
        assert_eq!(&rejection, &expected_rejection);

        let msg = rejection.to_string();
        let expected_msg = "server-side error, code 433: (unknown/generic server error)";
        assert_eq!(&msg, expected_msg);
    }
}
