use paqet_gui_lib::{ipc::IpcError, state::AppSnapshot};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const SNAPSHOT_FIXTURE: &str = include_str!("fixtures/ipc/app-snapshot.json");
const ERROR_FIXTURE: &str = include_str!("fixtures/ipc/error-profile-validation.json");

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
}

fn assert_typed_round_trip<T>(fixture: &str)
where
    T: DeserializeOwned + Serialize,
{
    let expected: Value = serde_json::from_str(fixture).unwrap();
    let typed: T = serde_json::from_str(fixture).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), expected);
}
