#![cfg(windows)]

use paqet_gui_lib::network::discover_interfaces;

#[test]
fn live_windows_discovery_returns_only_complete_sorted_interfaces() {
    let interfaces = discover_interfaces().expect("Windows network discovery should succeed");

    for interface in &interfaces {
        assert!(!interface.friendly_name.is_empty());
        assert_eq!(interface.interface_name, interface.friendly_name);
        assert!(interface.interface_name.len() <= 15);
        assert!(interface.guid.starts_with("\\Device\\NPF_{"));
        assert!(interface.guid.ends_with('}'));
        assert!(!interface.local_address.is_unspecified());
        assert!(!interface.gateway_address.is_unspecified());
        assert_eq!(interface.gateway_mac.len(), 17);
    }

    assert!(interfaces.windows(2).all(|pair| {
        pair[0].friendly_name.to_lowercase() <= pair[1].friendly_name.to_lowercase()
    }));
}
