use paqet_gui_lib::process::{
    FatalKind, LogClassification, OutputStream, PAQET_CONFIG_FLAG, PAQET_EXECUTABLE_NAME,
    PAQET_EXECUTABLE_SHA256, PAQET_EXECUTABLE_SIZE, PAQET_RUN_SUBCOMMAND, classify_output,
};
use serde_json::Value;

const CONTRACT: &str = include_str!("../compat/paqet-v1.0.0-alpha.20.json");
const SIDECAR_CONFIG: &str = include_str!("../tauri.sidecar.conf.json");
const STARTUP_SUCCESS: &str =
    include_str!("fixtures/paqet-v1.0.0-alpha.20/startup-success.stdout.log");
const CONNECTION_LOST: &str =
    include_str!("fixtures/paqet-v1.0.0-alpha.20/connection-lost.stdout.log");
const CONFIG_FAILURE: &str =
    include_str!("fixtures/paqet-v1.0.0-alpha.20/config-failure.stderr.log");
const CLIENT_FAILURE: &str =
    include_str!("fixtures/paqet-v1.0.0-alpha.20/client-failure.stdout.log");
const SHUTDOWN: &str = include_str!("fixtures/paqet-v1.0.0-alpha.20/shutdown.stdout.log");

#[test]
fn sidecar_resource_and_launch_contract_match_the_pinned_manifest() {
    let contract: Value = serde_json::from_str(CONTRACT).expect("contract must be valid JSON");
    let sidecar: Value =
        serde_json::from_str(SIDECAR_CONFIG).expect("sidecar config must be valid JSON");
    let external_binaries = sidecar["bundle"]["externalBin"]
        .as_array()
        .expect("externalBin must be an array");
    let artifact = &contract["windowsAmd64Artifact"];
    let sidecar_stem = artifact["tauriSidecarStem"]
        .as_str()
        .expect("sidecar stem must be text");
    let target_triple = artifact["targetTriple"]
        .as_str()
        .expect("target triple must be text");

    assert_eq!(external_binaries, &[Value::String(sidecar_stem.to_owned())]);
    assert_eq!(
        format!("{sidecar_stem}-{target_triple}.exe"),
        "binaries/paqet_windows_amd64-x86_64-pc-windows-msvc.exe"
    );
    assert_eq!(artifact["executableName"], PAQET_EXECUTABLE_NAME);
    assert_eq!(artifact["executableSize"], PAQET_EXECUTABLE_SIZE);
    assert_eq!(artifact["executableSha256"], PAQET_EXECUTABLE_SHA256);
    assert_eq!(
        contract["launch"]["arguments"],
        serde_json::json!([
            PAQET_RUN_SUBCOMMAND,
            PAQET_CONFIG_FLAG,
            contract["launch"]["defaultConfigArgument"]
        ])
    );
}

#[test]
fn pinned_fixture_classifications_match_the_machine_contract() {
    let contract: Value = serde_json::from_str(CONTRACT).expect("contract must be valid JSON");
    let fixtures = contract["fixtures"]
        .as_array()
        .expect("contract fixtures must be an array");

    for fixture in fixtures {
        let path = fixture["file"].as_str().expect("fixture path must be text");
        let stream = match fixture["stream"].as_str() {
            Some("stdout") => OutputStream::Stdout,
            Some("stderr") => OutputStream::Stderr,
            value => panic!("unsupported fixture stream: {value:?}"),
        };
        let expected = expected_classification(
            fixture["classification"]
                .as_str()
                .expect("fixture classification must be text"),
            stream,
        );
        let actual = fixture_contents(path)
            .lines()
            .map(|line| classify_output(stream, line))
            .collect::<Vec<_>>();
        let matched_line = matching_line(
            fixture_contents(path),
            fixture["match"]
                .as_str()
                .expect("fixture matcher must be text"),
        );

        assert!(
            actual.contains(&expected),
            "{path} classifications {actual:?} must include {expected:?}"
        );
        assert_eq!(
            actual
                .iter()
                .filter(|value| **value != LogClassification::Display)
                .count(),
            1,
            "{path} must contain exactly one lifecycle marker"
        );
        assert_eq!(
            classify_output(stream, matched_line),
            expected,
            "{path} manifest matcher must identify its lifecycle marker"
        );
    }
}

#[test]
fn startup_and_failure_golden_records_classify_line_by_line() {
    assert_eq!(
        classifications(OutputStream::Stdout, STARTUP_SUCCESS),
        vec![
            LogClassification::Display,
            LogClassification::Connected,
            LogClassification::Display,
        ]
    );
    assert_eq!(
        classifications(OutputStream::Stdout, CLIENT_FAILURE),
        vec![
            LogClassification::Display,
            LogClassification::Fatal {
                fatal_kind: FatalKind::Client,
            },
        ]
    );
    assert_eq!(
        classifications(OutputStream::Stderr, CONFIG_FAILURE),
        vec![
            LogClassification::Fatal {
                fatal_kind: FatalKind::Configuration,
            },
            LogClassification::Display,
            LogClassification::Display,
        ]
    );
}

fn classifications(stream: OutputStream, fixture: &str) -> Vec<LogClassification> {
    fixture
        .lines()
        .map(|line| classify_output(stream, line))
        .collect()
}

fn expected_classification(value: &str, stream: OutputStream) -> LogClassification {
    match value {
        "connected" => LogClassification::Connected,
        "connection-lost" => LogClassification::ConnectionLost,
        "fatal" => match stream {
            OutputStream::Stdout => LogClassification::Fatal {
                fatal_kind: FatalKind::Client,
            },
            OutputStream::Stderr => LogClassification::Fatal {
                fatal_kind: FatalKind::Configuration,
            },
        },
        "shutdown-requested" => LogClassification::ShutdownRequested,
        value => panic!("unsupported contract classification: {value}"),
    }
}

fn matching_line<'a>(fixture: &'a str, matcher: &str) -> &'a str {
    if let Some(expected) = matcher.strip_prefix("contains: ") {
        fixture
            .lines()
            .find(|line| line.contains(expected))
            .expect("contains matcher must identify a fixture line")
    } else if let Some(expected) = matcher.strip_prefix("exact: ") {
        fixture
            .lines()
            .find(|line| *line == expected)
            .expect("exact matcher must identify a fixture line")
    } else {
        panic!("unsupported fixture matcher: {matcher}");
    }
}

fn fixture_contents(path: &str) -> &'static str {
    match path {
        "tests/fixtures/paqet-v1.0.0-alpha.20/startup-success.stdout.log" => STARTUP_SUCCESS,
        "tests/fixtures/paqet-v1.0.0-alpha.20/connection-lost.stdout.log" => CONNECTION_LOST,
        "tests/fixtures/paqet-v1.0.0-alpha.20/config-failure.stderr.log" => CONFIG_FAILURE,
        "tests/fixtures/paqet-v1.0.0-alpha.20/client-failure.stdout.log" => CLIENT_FAILURE,
        "tests/fixtures/paqet-v1.0.0-alpha.20/shutdown.stdout.log" => SHUTDOWN,
        value => panic!("contract references unknown fixture: {value}"),
    }
}
