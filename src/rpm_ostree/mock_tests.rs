use crate::cincinnati::Cincinnati;
use crate::identity::Identity;
use mockito::{self, Matcher};
use std::collections::BTreeSet;
use tokio::runtime::current_thread as rt;

#[test]
fn test_simple_graph() {
    let simple_graph = r#"
{
  "nodes": [
    {
      "version": "0.0.0-mock",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "checksum",
        "org.fedoraproject.coreos.releases.age_index": "0"
      },
      "payload": "sha-mock"
    },
    {
      "version": "30.20190725.0",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "checksum",
        "org.fedoraproject.coreos.releases.age_index": "1"
      },
      "payload": "8b79877efa7ac06becd8637d95f8ca83aa385f89f383288bf3c2c31ca53216c7"
    }
  ],
  "edges": [
    [
      0,
      1
    ]
  ]
}
"#;

    let m_graph = mockito::mock("GET", Matcher::Regex(r"^/v1/graph?.+$".to_string()))
        .match_header("accept", Matcher::Regex("application/json".to_string()))
        .with_body(&simple_graph)
        .with_status(200)
        .create();

    let id = Identity::mock_default();
    let client = Cincinnati {
        base_url: mockito::server_url(),
    };
    let update = rt::block_on_all(client.fetch_update_hint(&id, BTreeSet::new(), true)).unwrap();
    m_graph.assert();

    let next = update.unwrap();
    assert_eq!(next.version, "30.20190725.0")
}
