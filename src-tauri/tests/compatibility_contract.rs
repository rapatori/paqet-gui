use serde_json::Value;

const CONTRACT: &str = include_str!("../compat/paqet-v1.0.0-alpha.20.json");

#[test]
fn pinned_release_contract_is_complete_and_consistent() {
    let contract: Value =
        serde_json::from_str(CONTRACT).expect("compatibility contract must be JSON");

    assert_eq!(contract["schemaVersion"], 1);
    assert_eq!(contract["upstream"]["version"], "v1.0.0-alpha.20");
    assert_eq!(
        contract["upstream"]["commit"],
        "f8ee6c130b6d44664e737419e99f7f677a6cf03a"
    );
    assert_eq!(
        contract["windowsAmd64Artifact"]["archiveSha256"],
        "2a59fec9d486d0d910f423cd33701adfc9de7ab9642565da7e05639ba9c18780"
    );
    assert_eq!(
        contract["windowsAmd64Artifact"]["executableSha256"],
        "49b377270473c223534ac1c2846d15c287863318e6fe6ee3c123f36ab97b441c"
    );
    assert_eq!(
        contract["windowsAmd64Artifact"]["authenticodeSigned"],
        false
    );
    assert_eq!(contract["applicationDefaults"]["logLevel"], "info");
    assert_eq!(
        contract["applicationAcceptedValues"]["logLevels"],
        json_array(&["debug", "info"])
    );
    assert_eq!(contract["applicationDefaults"]["transportProtocol"], "kcp");
    assert_eq!(
        contract["launch"]["arguments"],
        json_array(&["run", "-c", "config.yaml"])
    );

    let fixtures = contract["fixtures"]
        .as_array()
        .expect("fixtures must be an array");
    assert_eq!(fixtures.len(), 5);
    for fixture in fixtures {
        let relative_path = fixture["file"]
            .as_str()
            .expect("fixture file must be a string");
        let fixture_text = fixture_contents(relative_path);
        let matcher = fixture["match"]
            .as_str()
            .expect("fixture match must be a string");

        if let Some(expected) = matcher.strip_prefix("contains: ") {
            assert!(
                fixture_text.contains(expected),
                "{relative_path} must contain {expected:?}"
            );
        } else if let Some(expected) = matcher.strip_prefix("exact: ") {
            assert_eq!(
                fixture_text.trim_end(),
                expected,
                "{relative_path} must match"
            );
        } else {
            panic!("unsupported fixture matcher: {matcher}");
        }
    }
}

fn json_array(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|value| Value::from(*value)).collect())
}

fn fixture_contents(relative_path: &str) -> &'static str {
    match relative_path {
        "tests/fixtures/paqet-v1.0.0-alpha.20/startup-success.stdout.log" => {
            include_str!("fixtures/paqet-v1.0.0-alpha.20/startup-success.stdout.log")
        }
        "tests/fixtures/paqet-v1.0.0-alpha.20/connection-lost.stdout.log" => {
            include_str!("fixtures/paqet-v1.0.0-alpha.20/connection-lost.stdout.log")
        }
        "tests/fixtures/paqet-v1.0.0-alpha.20/config-failure.stderr.log" => {
            include_str!("fixtures/paqet-v1.0.0-alpha.20/config-failure.stderr.log")
        }
        "tests/fixtures/paqet-v1.0.0-alpha.20/client-failure.stdout.log" => {
            include_str!("fixtures/paqet-v1.0.0-alpha.20/client-failure.stdout.log")
        }
        "tests/fixtures/paqet-v1.0.0-alpha.20/shutdown.stdout.log" => {
            include_str!("fixtures/paqet-v1.0.0-alpha.20/shutdown.stdout.log")
        }
        _ => panic!("contract references an unknown fixture: {relative_path}"),
    }
}
