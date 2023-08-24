use crate::cincinnati::*;
use crate::identity::Identity;
use mockito::{self, Matcher};
use std::collections::BTreeSet;
use tokio::runtime as rt;

#[test]
fn test_empty_graph() {
    let mut server = mockito::Server::new();
    let empty_graph = r#"{ "nodes": [], "edges": [] }"#;
    let m_graph = server.mock("GET", Matcher::Regex(r"^/v1/graph?.+$".to_string()))
        .with_status(200)
        .with_header("accept", "application/json")
        .with_body(empty_graph)
        .create();
    
    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = Cincinnati {
        base_url: server.url(),
    };
    let update = runtime.block_on(client.next_update(&id, BTreeSet::new(), false));
    m_graph.assert();

    assert!(update.unwrap().is_none());
}
