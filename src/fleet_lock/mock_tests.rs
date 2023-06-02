use crate::fleet_lock::*;
use crate::identity::Identity;
use mockito::Matcher;
use tokio::runtime as rt;

#[test]
fn test_pre_reboot_lock() {
    let body = r#"
{
  "client_params": {
    "id": "e0f3745b108f471cbd4883c6fbed8cdd",
    "group": "mock-workers"
  }
}
"#;
    let m_pre_reboot = mockito::mock("POST", Matcher::Exact(format!("/{}", V1_PRE_REBOOT)))
        .match_header("fleet-lock-protocol", "true")
        .match_body(Matcher::PartialJsonString(body.to_string()))
        .with_status(200)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = ClientBuilder::new(mockito::server_url(), &id)
        .build()
        .unwrap();
    let res = runtime.block_on(client.pre_reboot());
    m_pre_reboot.assert();

    let lock = res.unwrap();
    assert!(lock);
}

#[test]
fn test_pre_reboot_error() {
    let body = r#"
{
  "kind": "f1",
  "value": "pre-reboot failure"
}
"#;
    let m_pre_reboot = mockito::mock("POST", Matcher::Exact(format!("/{}", V1_PRE_REBOOT)))
        .match_header("fleet-lock-protocol", "true")
        .with_status(404)
        .with_body(body)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = ClientBuilder::new(mockito::server_url(), &id)
        .build()
        .unwrap();
    let res = runtime.block_on(client.pre_reboot());
    m_pre_reboot.assert();

    let _rejection = res.unwrap_err();
}

#[test]
fn test_steady_state_lock() {
    let body = r#"
{
  "client_params": {
    "id": "e0f3745b108f471cbd4883c6fbed8cdd",
    "group": "mock-workers"
  }
}
"#;
    let m_steady_state = mockito::mock("POST", Matcher::Exact(format!("/{}", V1_STEADY_STATE)))
        .match_header("fleet-lock-protocol", "true")
        .match_body(Matcher::PartialJsonString(body.to_string()))
        .with_status(200)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = ClientBuilder::new(mockito::server_url(), &id)
        .build()
        .unwrap();
    let res = runtime.block_on(client.steady_state());
    m_steady_state.assert();

    let unlock = res.unwrap();
    assert!(unlock);
}

#[test]
fn test_steady_state_error() {
    let body = r#"
{
  "kind": "f1",
  "value": "pre-reboot failure"
}
"#;
    let m_steady_state = mockito::mock("POST", Matcher::Exact(format!("/{}", V1_STEADY_STATE)))
        .match_header("fleet-lock-protocol", "true")
        .with_status(404)
        .with_body(body)
        .create();

    let runtime = rt::Runtime::new().unwrap();
    let id = Identity::mock_default();
    let client = ClientBuilder::new(mockito::server_url(), &id)
        .build()
        .unwrap();
    let res = runtime.block_on(client.steady_state());
    m_steady_state.assert();

    let _rejection = res.unwrap_err();
}
