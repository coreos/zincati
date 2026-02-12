use crate::cincinnati::Cincinnati;
use crate::identity::Identity;
use mockito::{self, Matcher};
use std::collections::BTreeSet;
use tokio::runtime as rt;

#[test]
fn test_simple_graph() {
    let mut server = mockito::Server::new();
    let simple_graph = r#"
{
  "nodes": [
    {
      "version": "0.0.0-mock",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "oci",
        "org.fedoraproject.coreos.releases.age_index": "0"
      },
      "payload": "quay.io/fedora/fedora-coreos:oci-mock"
    },
    {
      "version": "43.20251120.3.0",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "oci",
        "org.fedoraproject.coreos.releases.age_index": "1"
      },
    "payload": "quay.io/fedora/fedora-coreos:latest"
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

    let m_graph = server
        .mock("GET", Matcher::Regex(r"^/v1/graph?.+$".to_string()))
        .match_header("accept", Matcher::Regex("application/json".to_string()))
        .with_body(simple_graph)
        .with_status(200)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = Cincinnati {
        base_url: server.url(),
    };
    let update = runtime.block_on(client.fetch_update_hint(&id, BTreeSet::new(), false));
    m_graph.assert();

    let next = update.unwrap();
    assert_eq!(next.version, "43.20251120.3.0")
}

#[test]
fn test_downgrade() {
    let mut server = mockito::Server::new();
    let simple_graph = r#"
{
  "nodes": [
    {
      "version": "43.20251120.3.0",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "oci",
        "org.fedoraproject.coreos.releases.age_index": "0"
      },
    "payload": "quay.io/fedora/fedora-coreos:downgrade"
    },
    {
      "version": "0.0.0-mock",
      "metadata": {
        "org.fedoraproject.coreos.scheme": "oci",
        "org.fedoraproject.coreos.releases.age_index": "1"
      },
      "payload": "quay.io/fedora/fedora-coreos:oci-mock"
    }
  ],
  "edges": [
    [
      1,
      0
    ]
  ]
}
"#;

    let m_graph = server
        .mock("GET", Matcher::Regex(r"^/v1/graph?.+$".to_string()))
        .match_header("accept", Matcher::Regex("application/json".to_string()))
        .with_body(simple_graph)
        .with_status(200)
        .expect(2)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = Cincinnati {
        base_url: server.url(),
    };

    // Downgrades denied.
    let upgrade = runtime.block_on(client.fetch_update_hint(&id, BTreeSet::new(), false));
    assert_eq!(upgrade, None);

    // Downgrades allowed.
    let downgrade = runtime.block_on(client.fetch_update_hint(&id, BTreeSet::new(), true));

    m_graph.assert();
    let next = downgrade.unwrap();
    assert_eq!(next.version, "43.20251120.3.0")
}
