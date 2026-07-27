use paqet_gui_lib::{
    ipc::IpcError,
    state::{AppSnapshot, RuntimeEvent},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const SNAPSHOT_FIXTURE: &str = include_str!("fixtures/ipc/app-snapshot.json");
const ERROR_FIXTURE: &str = include_str!("fixtures/ipc/error-profile-validation.json");
const CONFIG_ERROR_FIXTURE: &str = include_str!("fixtures/ipc/error-config-validation.json");
const RUNTIME_BOOTSTRAP_FIXTURE: &str = include_str!("fixtures/ipc/runtime-bootstrap.json");
const RUNTIME_OUTPUT_FIXTURE: &str = include_str!("fixtures/ipc/runtime-output.json");
const RUNTIME_GAP_FIXTURE: &str = include_str!("fixtures/ipc/runtime-gap.json");

#[test]
fn representative_snapshot_matches_the_rust_wire_contract() {
    assert_typed_round_trip::<AppSnapshot>(SNAPSHOT_FIXTURE);

    let value: Value = serde_json::from_str(SNAPSHOT_FIXTURE).unwrap();
    assert_eq!(value["revision"], "12");
    assert_eq!(value["advancedSettings"]["tcpBuffer"], "9007199254740993");
    assert_eq!(value["profiles"][0].get("encryptionKey"), None);
    assert_eq!(
        value["selectedProfile"]["encryptionKey"],
        "representative-test-key"
    );
}

#[test]
fn representative_error_matches_the_rust_wire_contract() {
    assert_typed_round_trip::<IpcError>(ERROR_FIXTURE);
    assert_typed_round_trip::<IpcError>(CONFIG_ERROR_FIXTURE);
}

#[test]
fn representative_runtime_events_match_the_rust_wire_contract() {
    for fixture in [
        RUNTIME_BOOTSTRAP_FIXTURE,
        RUNTIME_OUTPUT_FIXTURE,
        RUNTIME_GAP_FIXTURE,
    ] {
        assert_typed_round_trip::<RuntimeEvent>(fixture);
    }
}

fn assert_typed_round_trip<T>(fixture: &str)
where
    T: DeserializeOwned + Serialize,
{
    let expected: Value = serde_json::from_str(fixture).unwrap();
    let typed: T = serde_json::from_str(fixture).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), expected);
}
