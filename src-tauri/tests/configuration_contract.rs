use std::{
    fs,
    net::Ipv4Addr,
    sync::atomic::{AtomicU64, Ordering},
};

use paqet_gui_lib::{
    config::{
        AdvancedSettings, GeneratedConfig, KcpBlock, KcpMode, LogLevel, ManualKcpSettings,
        RuntimeConfigStore, generate,
    },
    network::NetworkInterface,
    profiles::{Profile, ProfileId},
};

const BASELINE: &str = include_str!("fixtures/paqet-v1.0.0-alpha.20/config-baseline.yaml");
const ALL_OVERRIDES: &str =
    include_str!("fixtures/paqet-v1.0.0-alpha.20/config-all-overrides.yaml");
const CONTRACT: &str = include_str!("../compat/paqet-v1.0.0-alpha.20.json");
const FIXTURE_KEY: &str = "fixture-key-not-used-by-a-real-server";
static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn generated_baseline_matches_pinned_golden_file() {
    let generated = generate(&profile(), &interface(), &AdvancedSettings::default()).unwrap();

    assert_eq!(written_yaml(&generated), BASELINE);
}

#[test]
fn generated_all_overrides_matches_pinned_golden_file() {
    let settings = AdvancedSettings {
        log_level: Some(LogLevel::Debug),
        pcap_socket_buffer: Some(8_388_608),
        local_tcp_flags: Some(vec!["PA".to_owned(), "S".to_owned()]),
        remote_tcp_flags: Some(vec!["RA".to_owned()]),
        connection_count: Some(4),
        tcp_buffer: Some(16_384),
        udp_buffer: Some(8192),
        kcp_mode: Some(KcpMode::Manual),
        manual_kcp: ManualKcpSettings {
            no_delay: Some(1),
            interval: Some(20),
            resend: Some(2),
            no_congestion: Some(1),
            write_delay: Some(false),
            ack_no_delay: Some(true),
        },
        kcp_mtu: Some(1400),
        kcp_receive_window: Some(1024),
        kcp_send_window: Some(1024),
        kcp_block: Some(KcpBlock::Null),
        smux_buffer: Some(8_388_608),
        stream_buffer: Some(4_194_304),
        smux_keepalive: Some(5),
        smux_timeout: Some(15),
    };
    let generated = generate(&profile(), &interface(), &settings).unwrap();

    assert_eq!(written_yaml(&generated), ALL_OVERRIDES);
}

#[test]
fn generated_values_remain_within_the_machine_readable_pin() {
    let contract: serde_json::Value = serde_json::from_str(CONTRACT).unwrap();
    let baseline: serde_yaml_ng::Value = serde_yaml_ng::from_str(BASELINE).unwrap();

    assert_eq!(
        baseline["role"].as_str(),
        contract["applicationDefaults"]["role"].as_str()
    );
    assert_eq!(
        baseline["log"]["level"].as_str(),
        contract["applicationDefaults"]["logLevel"].as_str()
    );
    assert_eq!(
        baseline["socks5"][0]["listen"].as_str(),
        contract["applicationDefaults"]["socks5Listen"].as_str()
    );
    assert_eq!(
        baseline["transport"]["protocol"].as_str(),
        contract["applicationDefaults"]["transportProtocol"].as_str()
    );

    let accepted_log_levels = &contract["applicationAcceptedValues"]["logLevels"];
    for level in [LogLevel::Debug, LogLevel::Info] {
        assert!(
            accepted_log_levels
                .as_array()
                .unwrap()
                .contains(&serde_json::to_value(level).unwrap())
        );
    }

    let accepted_modes = &contract["acceptedValues"]["kcpModes"];
    for mode in [
        KcpMode::Normal,
        KcpMode::Fast,
        KcpMode::Fast2,
        KcpMode::Fast3,
        KcpMode::Manual,
    ] {
        assert!(
            accepted_modes
                .as_array()
                .unwrap()
                .contains(&serde_json::to_value(mode).unwrap())
        );
    }

    let accepted_blocks = &contract["acceptedValues"]["kcpBlocks"];
    for block in [
        KcpBlock::Aes,
        KcpBlock::Aes128,
        KcpBlock::Aes128Gcm,
        KcpBlock::Aes192,
        KcpBlock::Salsa20,
        KcpBlock::Blowfish,
        KcpBlock::Twofish,
        KcpBlock::Cast5,
        KcpBlock::TripleDes,
        KcpBlock::Tea,
        KcpBlock::Xtea,
        KcpBlock::Xor,
        KcpBlock::Sm4,
        KcpBlock::None,
        KcpBlock::Null,
    ] {
        assert!(
            accepted_blocks
                .as_array()
                .unwrap()
                .contains(&serde_json::to_value(block).unwrap())
        );
    }
}

fn profile() -> Profile {
    Profile {
        id: ProfileId::new(),
        name: "Fixture".to_owned(),
        server_host: "198.51.100.10".to_owned(),
        port: 9999,
        encryption_key: FIXTURE_KEY.to_owned(),
    }
}

fn interface() -> NetworkInterface {
    NetworkInterface {
        friendly_name: "Ethernet".to_owned(),
        interface_name: "Ethernet".to_owned(),
        guid: "\\Device\\NPF_{12345678-1234-1234-1234-123456789ABC}".to_owned(),
        local_address: Ipv4Addr::new(192, 0, 2, 10),
        gateway_address: Ipv4Addr::new(192, 0, 2, 1),
        gateway_mac: "00:11:22:AA:BB:CC".to_owned(),
    }
}

fn written_yaml(config: &GeneratedConfig) -> String {
    let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "paqet-gui-contract-test-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("config.yaml");
    RuntimeConfigStore::new(path.clone()).write(config).unwrap();
    let yaml = fs::read_to_string(path).unwrap();
    fs::remove_dir_all(directory).unwrap();
    yaml
}
