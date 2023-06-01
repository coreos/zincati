use crate::cincinnati::*;
use crate::identity::Identity;
use mockito::{self, Matcher};
use std::collections::BTreeSet;
use tokio::runtime as rt;

#[test]
fn test_empty_graph() {
    let empty_graph = r#"{ "nodes": [], "edges": [] }"#;
    let m_graph = mockito::mock("GET", Matcher::Regex(r"^/v1/graph?.+$".to_string()))
        .match_header("accept", Matcher::Regex("application/json".to_string()))
        .with_body(empty_graph)
        .with_status(200)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = Cincinnati {
        base_url: mockito::server_url(),
    };
    let update = runtime.block_on(client.next_update(&id, BTreeSet::new(), false));
    m_graph.assert();

    assert!(update.unwrap().is_none());
}
