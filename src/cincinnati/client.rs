//! Asynchronous Cincinnati client.
//!
//! This client implements the [Cincinnati protocol] for update-hints.
//!
//! [Cincinnati protocol]: https://github.com/openshift/cincinnati/blob/master/docs/design/cincinnati.md#graph-api

// TODO(lucab): eventually move to its own "cincinnati client library" crate

#![allow(unused)]

use failure::{Error, Fallible, ResultExt};
use futures::future;
use futures::prelude::*;
use reqwest::r#async as asynchro;
use reqwest::Method;
use serde::Deserialize;
use std::collections::HashMap;

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
    pub fn fetch_graph(&self) -> impl Future<Item = Graph, Error = Error> {
        let req = self.new_request(Method::GET, V1_GRAPH_PATH);
        future::result(req)
            .and_then(|req| req.send().from_err())
            .and_then(|resp| resp.error_for_status().map_err(Error::from))
            .and_then(|mut resp| resp.json::<Graph>().from_err())
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
            .header("content-type", "application/json")
            .query(&self.query_params);
        Ok(builder)
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
            None => asynchro::ClientBuilder::new().build()?,
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
