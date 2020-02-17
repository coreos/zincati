//! Asynchronous Cincinnati client.
//!
//! This client implements the [Cincinnati protocol] for update-hints.
//!
//! [Cincinnati protocol]: https://github.com/openshift/cincinnati/blob/master/docs/design/cincinnati.md#graph-api

// TODO(lucab): eventually move to its own "cincinnati client library" crate

#![allow(unused)]

use failure::{format_err, Error, Fail, Fallible, ResultExt};
use futures::future;
use futures::prelude::*;
use reqwest::r#async as asynchro;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Default timeout for HTTP requests completion (30 minutes).
const DEFAULT_HTTP_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Cincinnati graph API path endpoint (v1).
static V1_GRAPH_PATH: &str = "v1/graph";

/// Cincinnati JSON protocol: node object.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub version: String,
    pub payload: String,
    pub metadata: HashMap<String, String>,
}

/// Cincinnati JSON protocol: graph object.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<(u64, u64)>,
}

/// Cincinnati JSON protocol: service error.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GraphJSONError {
    /// Machine-friendly brief error kind.
    pub(crate) kind: String,
    /// Human-friendly detailed error explanation.
    pub(crate) value: String,
}

/// Error related to the Cincinnati service.
#[derive(Clone, Debug, Fail, PartialEq, Eq)]
pub enum CincinnatiError {
    /// Graph endpoint error.
    Graph(reqwest::StatusCode, GraphJSONError),
    /// Generic HTTP error.
    HTTP(reqwest::StatusCode),
    /// Client builder failed.
    FailedClientBuilder(String),
    /// Client failed JSON decoding.
    FailedJSONDecoding(String),
    /// Failed to lookup node in graph.
    FailedNodeLookup(String),
    /// Failed parsing node from graph.
    FailedNodeParsing(String),
    /// Client failed request.
    FailedRequest(String),
}

impl CincinnatiError {
    /// Return the machine-friendly brief error kind.
    pub fn error_kind(&self) -> String {
        match *self {
            CincinnatiError::Graph(_, ref err) => err.kind.clone(),
            CincinnatiError::HTTP(status) => format!("generic_http_{}", status.as_u16()),
            CincinnatiError::FailedClientBuilder(_) => "client_failed_build".to_string(),
            CincinnatiError::FailedJSONDecoding(_) => "client_failed_json_decoding".to_string(),
            CincinnatiError::FailedNodeLookup(_) => "client_failed_node_lookup".to_string(),
            CincinnatiError::FailedNodeParsing(_) => "client_failed_node_parsing".to_string(),
            CincinnatiError::FailedRequest(_) => "client_failed_request".to_string(),
        }
    }

    /// Return the human-friendly detailed error explanation.
    pub fn error_value(&self) -> String {
        match *self {
            CincinnatiError::Graph(_, ref err) => err.value.clone(),
            CincinnatiError::HTTP(_) => "(unknown/generic server error)".to_string(),
            CincinnatiError::FailedClientBuilder(ref err)
            | CincinnatiError::FailedJSONDecoding(ref err)
            | CincinnatiError::FailedNodeLookup(ref err)
            | CincinnatiError::FailedNodeParsing(ref err)
            | CincinnatiError::FailedRequest(ref err) => err.clone(),
        }
    }

    /// Return the server-side error status code, if any.
    pub fn status_code(&self) -> Option<u16> {
        match *self {
            CincinnatiError::Graph(s, _) | CincinnatiError::HTTP(s) => Some(s.as_u16()),
            _ => None,
        }
    }
}

impl std::fmt::Display for CincinnatiError {
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
#[derive(Clone, Debug)]
pub struct Client {
    /// Base URL for API endpoint.
    api_base: reqwest::Url,
    /// Asynchronous reqwest client.
    hclient: asynchro::Client,
    /// Client parameters (query portion).
    query_params: HashMap<String, String>,
}

impl Client {
    /// Fetch an update-graph from Cincinnati.
    pub fn fetch_graph(&self) -> Box<dyn Future<Item = Graph, Error = CincinnatiError>> {
        let req = self.new_request(Method::GET, V1_GRAPH_PATH);
        let result = future::result(req)
            .and_then(|req| req.send().from_err())
            .map_err(move |e| CincinnatiError::FailedRequest(e.to_string()))
            .and_then(Self::map_response);
        Box::new(result)
    }

    /// Return a request builder with base URL and parameters set.
    fn new_request<S: AsRef<str>>(
        &self,
        method: reqwest::Method,
        url_suffix: S,
    ) -> Fallible<asynchro::RequestBuilder> {
        let url = self.api_base.clone().join(url_suffix.as_ref())?;
        let builder = self
            .hclient
            .request(method, url)
            .header("accept", "application/json")
            .query(&self.query_params);
        Ok(builder)
    }

    /// Map an HTTP response to a service result.
    fn map_response(
        mut response: asynchro::Response,
    ) -> Box<dyn Future<Item = Graph, Error = CincinnatiError>> {
        let status = response.status();

        // On success, try to decode graph.
        if status.is_success() {
            let result = response.json::<Graph>().map_err(move |e| {
                CincinnatiError::FailedJSONDecoding(format!("failed to decode graph: {}", e))
            });
            return Box::new(result);
        }

        // On error, decode failure details (or synthesize a generic error).
        let error = response.json::<GraphJSONError>().then(move |r| {
            if let Ok(mut rej) = r {
                Err(CincinnatiError::Graph(status, rej))
            } else {
                Err(CincinnatiError::HTTP(status))
            }
        });
        Box::new(error)
    }
}

/// Client builder.
#[derive(Clone, Debug)]
pub struct ClientBuilder {
    /// Base URL for API endpoint (mandatory).
    api_base: String,
    /// Asynchronous reqwest client (custom).
    hclient: Option<asynchro::Client>,
    /// Client parameters (custom).
    query_params: Option<HashMap<String, String>>,
}

impl ClientBuilder {
    /// Return a new builder for the given base API endpoint URL.
    pub fn new<T>(api_base: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            api_base: api_base.into(),
            hclient: None,
            query_params: None,
        }
    }

    /// Set (or reset) the query parameters to use.
    pub fn query_params(self, params: Option<HashMap<String, String>>) -> Self {
        let mut builder = self;
        builder.query_params = params;
        builder
    }

    /// Set (or reset) the HTTP client to use.
    pub fn http_client(self, hclient: Option<asynchro::Client>) -> Self {
        let mut builder = self;
        builder.hclient = hclient;
        builder
    }

    /// Build a client with specified parameters.
    pub fn build(self) -> Fallible<Client> {
        let hclient = match self.hclient {
            Some(client) => client,
            None => asynchro::ClientBuilder::new()
                .use_sys_proxy()
                .timeout(DEFAULT_HTTP_COMPLETION_TIMEOUT)
                .build()?,
        };
        let query_params = match self.query_params {
            Some(params) => params,
            None => HashMap::new(),
        };

        let api_base = reqwest::Url::parse(&self.api_base)
            .context(format!("failed to parse '{}'", &self.api_base))?;
        let client = Client {
            api_base,
            hclient,
            query_params,
        };
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::response::Response;
    use http::status::StatusCode;
    use tokio::runtime::current_thread as rt;

    #[test]
    fn test_graph_server_error_display() {
        let err_body = r#"
{
  "kind": "failure_foo",
  "value": "failed to perform foo"
}
"#;
        let response = Response::builder().status(466).body(err_body).unwrap();
        let fut_rejection = Client::map_response(response.into());
        let rejection = rt::block_on_all(fut_rejection).unwrap_err();
        let expected_rejection = CincinnatiError::Graph(
            StatusCode::from_u16(466).unwrap(),
            GraphJSONError {
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
    fn test_graph_http_error_display() {
        let response = Response::builder().status(433).body("").unwrap();
        let fut_rejection = Client::map_response(response.into());
        let rejection = rt::block_on_all(fut_rejection).unwrap_err();
        let expected_rejection = CincinnatiError::HTTP(StatusCode::from_u16(433).unwrap());
        assert_eq!(&rejection, &expected_rejection);

        let msg = rejection.to_string();
        let expected_msg = "server-side error, code 433: (unknown/generic server error)";
        assert_eq!(&msg, expected_msg);
    }

    #[test]
    fn test_graph_client_error_display() {
        let response = Response::builder().status(200).body("{}").unwrap();
        let fut_rejection = Client::map_response(response.into());
        let rejection = rt::block_on_all(fut_rejection).unwrap_err();
        let expected_rejection = CincinnatiError::FailedJSONDecoding(
            "failed to decode graph: missing field `nodes` at line 1 column 2".to_string(),
        );
        assert_eq!(&rejection, &expected_rejection);

        let msg = rejection.to_string();
        let expected_msg =
            "client-side error: failed to decode graph: missing field `nodes` at line 1 column 2";
        assert_eq!(&msg, expected_msg);
    }
}
