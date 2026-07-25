use std::{fmt, net::Ipv4Addr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub friendly_name: String,
    pub interface_name: String,
    pub guid: String,
    pub local_address: Ipv4Addr,
    pub gateway_address: Ipv4Addr,
    pub gateway_mac: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkError {
    UnsupportedPlatform,
    WindowsApi { operation: &'static str, code: u32 },
    InvalidWindowsData(&'static str),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("network discovery is only supported on Windows")
            }
            Self::WindowsApi { operation, code } => {
                write!(formatter, "{operation} failed with Windows error {code}")
            }
            Self::InvalidWindowsData(context) => {
                write!(formatter, "Windows returned invalid {context}")
            }
        }
    }
}

impl std::error::Error for NetworkError {}

pub fn discover_interfaces() -> Result<Vec<NetworkInterface>, NetworkError> {
    #[cfg(windows)]
    {
        let snapshot = windows::snapshot()?;
        Ok(select_interfaces(
            &snapshot.adapters,
            &snapshot.routes,
            &snapshot.neighbors,
        ))
    }

    #[cfg(not(windows))]
    {
        Err(NetworkError::UnsupportedPlatform)
    }
}

#[derive(Clone, Debug)]
struct Adapter {
    luid: u64,
    index: u32,
    friendly_name: String,
    adapter_name: String,
    interface_type: InterfaceType,
    is_up: bool,
    ipv4_metric: u32,
    addresses: Vec<AdapterAddress>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InterfaceType {
    Ethernet,
    WiFi,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct AdapterAddress {
    address: Ipv4Addr,
    prefix_length: u8,
    is_preferred: bool,
}

#[derive(Clone, Copy, Debug)]
struct Route {
    luid: u64,
    index: u32,
    destination: Ipv4Addr,
    prefix_length: u8,
    next_hop: Ipv4Addr,
    metric: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum NeighborState {
    Permanent,
    Reachable,
    Stale,
    Delay,
    Probe,
    Unusable,
}

#[derive(Clone, Debug)]
struct Neighbor {
    luid: u64,
    index: u32,
    address: Ipv4Addr,
    mac: Vec<u8>,
    state: NeighborState,
}

fn select_interfaces(
    adapters: &[Adapter],
    routes: &[Route],
    neighbors: &[Neighbor],
) -> Vec<NetworkInterface> {
    let mut interfaces = adapters
        .iter()
        .filter_map(|adapter| select_interface(adapter, routes, neighbors))
        .collect::<Vec<_>>();

    interfaces.sort_by(|left, right| {
        left.friendly_name
            .to_lowercase()
            .cmp(&right.friendly_name.to_lowercase())
            .then_with(|| {
                left.local_address
                    .octets()
                    .cmp(&right.local_address.octets())
            })
            .then_with(|| left.guid.cmp(&right.guid))
    });
    interfaces
}

fn select_interface(
    adapter: &Adapter,
    routes: &[Route],
    neighbors: &[Neighbor],
) -> Option<NetworkInterface> {
    if !adapter.is_up
        || !matches!(
            adapter.interface_type,
            InterfaceType::Ethernet | InterfaceType::WiFi
        )
        || adapter.friendly_name.is_empty()
        || adapter.friendly_name.len() > 15
    {
        return None;
    }

    let guid = npcap_guid(&adapter.adapter_name)?;
    let mut candidates = routes
        .iter()
        .filter(|route| {
            same_interface(adapter.luid, adapter.index, route.luid, route.index)
                && route.prefix_length == 0
                && route.destination.is_unspecified()
                && usable_gateway(route.next_hop)
        })
        .filter_map(|route| {
            let address = select_source_address(&adapter.addresses, route.next_hop)?;
            let neighbor = select_neighbor(adapter, route.next_hop, neighbors)?;
            Some((
                route.metric.saturating_add(adapter.ipv4_metric),
                neighbor.state,
                route.next_hop,
                address,
                neighbor,
            ))
        })
        .collect::<Vec<_>>();

    candidates.sort_by_key(|(metric, state, gateway, address, _)| {
        (
            *metric,
            *state,
            gateway.octets(),
            !address.is_preferred,
            std::cmp::Reverse(address.prefix_length),
            address.address.octets(),
        )
    });
    let (_, _, gateway_address, local, neighbor) = candidates.first()?;

    Some(NetworkInterface {
        friendly_name: adapter.friendly_name.clone(),
        interface_name: adapter.friendly_name.clone(),
        guid,
        local_address: local.address,
        gateway_address: *gateway_address,
        gateway_mac: format_mac(&neighbor.mac),
    })
}

fn select_neighbor<'a>(
    adapter: &Adapter,
    gateway: Ipv4Addr,
    neighbors: &'a [Neighbor],
) -> Option<&'a Neighbor> {
    let mut candidates = neighbors
        .iter()
        .filter(|neighbor| {
            same_interface(adapter.luid, adapter.index, neighbor.luid, neighbor.index)
                && neighbor.address == gateway
                && neighbor.state != NeighborState::Unusable
                && usable_mac(&neighbor.mac)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|neighbor| (neighbor.state, neighbor.mac.as_slice()));
    candidates.first().copied()
}

fn same_interface(left_luid: u64, left_index: u32, right_luid: u64, right_index: u32) -> bool {
    if left_luid != 0 && right_luid != 0 {
        left_luid == right_luid
    } else {
        left_index != 0 && left_index == right_index
    }
}

fn select_source_address(
    addresses: &[AdapterAddress],
    gateway: Ipv4Addr,
) -> Option<AdapterAddress> {
    let mut candidates = addresses
        .iter()
        .copied()
        .filter(|candidate| {
            usable_local_address(candidate.address)
                && candidate.prefix_length <= 32
                && same_subnet(candidate.address, gateway, candidate.prefix_length)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        (
            !candidate.is_preferred,
            std::cmp::Reverse(candidate.prefix_length),
            candidate.address.octets(),
        )
    });
    candidates.first().copied()
}

fn usable_local_address(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_multicast()
        && !address.is_broadcast()
}

fn usable_gateway(address: Ipv4Addr) -> bool {
    usable_local_address(address)
}

fn same_subnet(left: Ipv4Addr, right: Ipv4Addr, prefix_length: u8) -> bool {
    let mask = if prefix_length == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_length)
    };
    u32::from(left) & mask == u32::from(right) & mask
}

fn usable_mac(mac: &[u8]) -> bool {
    mac.len() == 6 && mac.iter().any(|octet| *octet != 0) && mac[0] & 1 == 0
}

fn format_mac(mac: &[u8]) -> String {
    mac.iter()
        .map(|octet| format!("{octet:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn npcap_guid(adapter_name: &str) -> Option<String> {
    let value = adapter_name
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))?;
    let guid = Uuid::parse_str(value).ok()?;
    Some(format!(
        "\\Device\\NPF_{{{}}}",
        guid.hyphenated().to_string().to_uppercase()
    ))
}

#[cfg(windows)]
mod windows {
    use std::{ffi::CStr, mem, net::Ipv4Addr, ptr, slice};

    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, NO_ERROR},
        NetworkManagement::{
            IpHelper::{
                FreeMibTable, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
                GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetIpForwardTable2, GetIpNetTable2,
                IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IP_ADAPTER_ADDRESSES_LH,
                MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2, MIB_IPNET_ROW2, MIB_IPNET_TABLE2,
            },
            Ndis::IfOperStatusUp,
        },
        Networking::WinSock::{
            AF_INET, IpDadStateDeprecated as IP_DAD_STATE_DEPRECATED,
            IpDadStatePreferred as IP_DAD_STATE_PREFERRED, NlnsDelay as NLNS_DELAY,
            NlnsPermanent as NLNS_PERMANENT, NlnsProbe as NLNS_PROBE,
            NlnsReachable as NLNS_REACHABLE, NlnsStale as NLNS_STALE, SOCKADDR, SOCKADDR_IN,
            SOCKADDR_INET,
        },
    };

    use super::{
        Adapter, AdapterAddress, InterfaceType, Neighbor, NeighborState, NetworkError, Route,
    };

    const INITIAL_ADAPTER_BUFFER_SIZE: usize = 15 * 1024;
    const MAX_ADAPTER_BUFFER_ATTEMPTS: usize = 3;

    pub(super) struct Snapshot {
        pub adapters: Vec<Adapter>,
        pub routes: Vec<Route>,
        pub neighbors: Vec<Neighbor>,
    }

    pub(super) fn snapshot() -> Result<Snapshot, NetworkError> {
        Ok(Snapshot {
            adapters: adapters()?,
            routes: routes()?,
            neighbors: neighbors()?,
        })
    }

    fn adapters() -> Result<Vec<Adapter>, NetworkError> {
        let mut byte_length = INITIAL_ADAPTER_BUFFER_SIZE;
        for _ in 0..MAX_ADAPTER_BUFFER_ATTEMPTS {
            let word_length = byte_length.div_ceil(mem::size_of::<usize>());
            let mut buffer = vec![0usize; word_length];
            let mut api_length = u32::try_from(buffer.len() * mem::size_of::<usize>())
                .map_err(|_| NetworkError::InvalidWindowsData("adapter buffer size"))?;
            let result = unsafe {
                GetAdaptersAddresses(
                    u32::from(AF_INET),
                    GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                    ptr::null(),
                    buffer.as_mut_ptr().cast(),
                    &mut api_length,
                )
            };

            if result == ERROR_BUFFER_OVERFLOW {
                byte_length = usize::try_from(api_length)
                    .map_err(|_| NetworkError::InvalidWindowsData("adapter buffer size"))?;
                continue;
            }
            if result == ERROR_NO_DATA {
                return Ok(Vec::new());
            }
            if result != NO_ERROR {
                return Err(NetworkError::WindowsApi {
                    operation: "GetAdaptersAddresses",
                    code: result,
                });
            }

            return unsafe { copy_adapters(buffer.as_ptr().cast()) };
        }

        Err(NetworkError::WindowsApi {
            operation: "GetAdaptersAddresses",
            code: ERROR_BUFFER_OVERFLOW,
        })
    }

    unsafe fn copy_adapters(
        mut current: *const IP_ADAPTER_ADDRESSES_LH,
    ) -> Result<Vec<Adapter>, NetworkError> {
        let mut adapters = Vec::new();
        while let Some(row) = unsafe { current.as_ref() } {
            let friendly_name = unsafe { wide_string(row.FriendlyName) }?;
            let adapter_name = unsafe { narrow_string(row.AdapterName) }?;
            let mut addresses = Vec::new();
            let mut unicast = row.FirstUnicastAddress;
            while let Some(address) = unsafe { unicast.as_ref() } {
                if matches!(
                    address.DadState,
                    IP_DAD_STATE_PREFERRED | IP_DAD_STATE_DEPRECATED
                ) && let Some(ipv4) = unsafe { socket_address_ipv4(address.Address.lpSockaddr) }
                {
                    addresses.push(AdapterAddress {
                        address: ipv4,
                        prefix_length: address.OnLinkPrefixLength,
                        is_preferred: address.DadState == IP_DAD_STATE_PREFERRED,
                    });
                }
                unicast = address.Next;
            }
            let interface_type = match row.IfType {
                IF_TYPE_ETHERNET_CSMACD => InterfaceType::Ethernet,
                IF_TYPE_IEEE80211 => InterfaceType::WiFi,
                _ => InterfaceType::Other,
            };
            adapters.push(Adapter {
                luid: unsafe { row.Luid.Value },
                index: unsafe { row.Anonymous1.Anonymous.IfIndex },
                friendly_name,
                adapter_name,
                interface_type,
                is_up: row.OperStatus == IfOperStatusUp,
                ipv4_metric: row.Ipv4Metric,
                addresses,
            });
            current = row.Next;
        }
        Ok(adapters)
    }

    fn routes() -> Result<Vec<Route>, NetworkError> {
        let table =
            MibTable::<MIB_IPFORWARD_TABLE2>::load("GetIpForwardTable2", |output| unsafe {
                GetIpForwardTable2(AF_INET, output)
            })?;
        let rows = unsafe {
            flexible_rows(
                &*table.pointer,
                (*table.pointer).NumEntries,
                ptr::addr_of!((*table.pointer).Table).cast::<MIB_IPFORWARD_ROW2>(),
            )?
        };
        Ok(rows
            .iter()
            .filter_map(|row| {
                if row.Loopback {
                    return None;
                }
                Some(Route {
                    luid: unsafe { row.InterfaceLuid.Value },
                    index: row.InterfaceIndex,
                    destination: sockaddr_inet_ipv4(&row.DestinationPrefix.Prefix)?,
                    prefix_length: row.DestinationPrefix.PrefixLength,
                    next_hop: sockaddr_inet_ipv4(&row.NextHop)?,
                    metric: row.Metric,
                })
            })
            .collect())
    }

    fn neighbors() -> Result<Vec<Neighbor>, NetworkError> {
        let table = MibTable::<MIB_IPNET_TABLE2>::load("GetIpNetTable2", |output| unsafe {
            GetIpNetTable2(AF_INET, output)
        })?;
        let rows = unsafe {
            flexible_rows(
                &*table.pointer,
                (*table.pointer).NumEntries,
                ptr::addr_of!((*table.pointer).Table).cast::<MIB_IPNET_ROW2>(),
            )?
        };
        Ok(rows
            .iter()
            .filter_map(|row| {
                let length = usize::try_from(row.PhysicalAddressLength).ok()?;
                let mac = row.PhysicalAddress.get(..length)?.to_vec();
                let state = match row.State {
                    NLNS_PERMANENT => NeighborState::Permanent,
                    NLNS_REACHABLE => NeighborState::Reachable,
                    NLNS_STALE => NeighborState::Stale,
                    NLNS_DELAY => NeighborState::Delay,
                    NLNS_PROBE => NeighborState::Probe,
                    _ => NeighborState::Unusable,
                };
                Some(Neighbor {
                    luid: unsafe { row.InterfaceLuid.Value },
                    index: row.InterfaceIndex,
                    address: sockaddr_inet_ipv4(&row.Address)?,
                    mac,
                    state,
                })
            })
            .collect())
    }

    struct MibTable<T> {
        pointer: *mut T,
    }

    impl<T> MibTable<T> {
        fn load(
            operation: &'static str,
            load: impl FnOnce(*mut *mut T) -> u32,
        ) -> Result<Self, NetworkError> {
            let mut pointer = ptr::null_mut();
            let result = load(&mut pointer);
            if result != NO_ERROR {
                return Err(NetworkError::WindowsApi {
                    operation,
                    code: result,
                });
            }
            if pointer.is_null() {
                return Err(NetworkError::InvalidWindowsData("IP Helper table pointer"));
            }
            Ok(Self { pointer })
        }
    }

    impl<T> Drop for MibTable<T> {
        fn drop(&mut self) {
            unsafe { FreeMibTable(self.pointer.cast()) };
        }
    }

    unsafe fn flexible_rows<T, R>(
        _table: &T,
        count: u32,
        rows: *const R,
    ) -> Result<&[R], NetworkError> {
        let count = usize::try_from(count)
            .map_err(|_| NetworkError::InvalidWindowsData("IP Helper table length"))?;
        Ok(unsafe { slice::from_raw_parts(rows, count) })
    }

    unsafe fn wide_string(pointer: *const u16) -> Result<String, NetworkError> {
        if pointer.is_null() {
            return Err(NetworkError::InvalidWindowsData("adapter friendly name"));
        }
        let mut length = 0usize;
        while unsafe { *pointer.add(length) } != 0 {
            length = length
                .checked_add(1)
                .ok_or(NetworkError::InvalidWindowsData("adapter friendly name"))?;
        }
        String::from_utf16(unsafe { slice::from_raw_parts(pointer, length) })
            .map_err(|_| NetworkError::InvalidWindowsData("adapter friendly name"))
    }

    unsafe fn narrow_string(pointer: *const u8) -> Result<String, NetworkError> {
        if pointer.is_null() {
            return Err(NetworkError::InvalidWindowsData("adapter name"));
        }
        unsafe { CStr::from_ptr(pointer.cast()) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| NetworkError::InvalidWindowsData("adapter name"))
    }

    unsafe fn socket_address_ipv4(pointer: *const SOCKADDR) -> Option<Ipv4Addr> {
        let sockaddr = unsafe { pointer.as_ref() }?;
        if sockaddr.sa_family != AF_INET {
            return None;
        }
        let ipv4 = unsafe { &*pointer.cast::<SOCKADDR_IN>() };
        Some(ipv4_address(unsafe { ipv4.sin_addr.S_un.S_un_b }))
    }

    fn sockaddr_inet_ipv4(address: &SOCKADDR_INET) -> Option<Ipv4Addr> {
        if unsafe { address.si_family } != AF_INET {
            return None;
        }
        Some(ipv4_address(unsafe { address.Ipv4.sin_addr.S_un.S_un_b }))
    }

    fn ipv4_address(bytes: windows_sys::Win32::Networking::WinSock::IN_ADDR_0_0) -> Ipv4Addr {
        Ipv4Addr::new(bytes.s_b1, bytes.s_b2, bytes.s_b3, bytes.s_b4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADAPTER_GUID: &str = "{12345678-1234-5678-9ABC-1234567890AB}";

    fn adapter(name: &str) -> Adapter {
        Adapter {
            luid: 10,
            index: 7,
            friendly_name: name.to_owned(),
            adapter_name: ADAPTER_GUID.to_owned(),
            interface_type: InterfaceType::Ethernet,
            is_up: true,
            ipv4_metric: 25,
            addresses: vec![AdapterAddress {
                address: Ipv4Addr::new(192, 168, 1, 20),
                prefix_length: 24,
                is_preferred: true,
            }],
        }
    }

    fn route(metric: u32, gateway: Ipv4Addr) -> Route {
        Route {
            luid: 10,
            index: 7,
            destination: Ipv4Addr::UNSPECIFIED,
            prefix_length: 0,
            next_hop: gateway,
            metric,
        }
    }

    fn neighbor(gateway: Ipv4Addr, state: NeighborState) -> Neighbor {
        Neighbor {
            luid: 10,
            index: 7,
            address: gateway,
            mac: vec![0x00, 0x11, 0x22, 0xAA, 0xBB, 0xCC],
            state,
        }
    }

    #[test]
    fn selects_a_complete_paqet_ready_interface() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);

        let selected = select_interfaces(
            &[adapter("Ethernet")],
            &[route(10, gateway)],
            &[neighbor(gateway, NeighborState::Reachable)],
        );

        assert_eq!(
            selected,
            vec![NetworkInterface {
                friendly_name: "Ethernet".to_owned(),
                interface_name: "Ethernet".to_owned(),
                guid: "\\Device\\NPF_{12345678-1234-5678-9ABC-1234567890AB}".to_owned(),
                local_address: Ipv4Addr::new(192, 168, 1, 20),
                gateway_address: gateway,
                gateway_mac: "00:11:22:AA:BB:CC".to_owned(),
            }]
        );
    }

    #[test]
    fn filters_unsupported_down_and_overlong_interfaces() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        let mut down = adapter("Ethernet");
        down.is_up = false;
        let mut tunnel = adapter("Tunnel");
        tunnel.interface_type = InterfaceType::Other;
        let unicode_over_bytes = adapter("éééééééé");
        assert_eq!(unicode_over_bytes.friendly_name.chars().count(), 8);
        assert_eq!(unicode_over_bytes.friendly_name.len(), 16);

        let selected = select_interfaces(
            &[
                down,
                tunnel,
                adapter("Interface name is too long"),
                unicode_over_bytes,
            ],
            &[route(10, gateway)],
            &[neighbor(gateway, NeighborState::Reachable)],
        );

        assert!(selected.is_empty());
    }

    #[test]
    fn chooses_the_lowest_metric_complete_default_route() {
        let high_gateway = Ipv4Addr::new(192, 168, 1, 1);
        let low_gateway = Ipv4Addr::new(192, 168, 1, 2);

        let selected = select_interfaces(
            &[adapter("Ethernet")],
            &[route(30, high_gateway), route(5, low_gateway)],
            &[
                neighbor(high_gateway, NeighborState::Reachable),
                neighbor(low_gateway, NeighborState::Stale),
            ],
        );

        assert_eq!(selected[0].gateway_address, low_gateway);
    }

    #[test]
    fn ignores_routes_without_a_usable_neighbor() {
        let unresolved_gateway = Ipv4Addr::new(192, 168, 1, 1);
        let resolved_gateway = Ipv4Addr::new(192, 168, 1, 2);

        let selected = select_interfaces(
            &[adapter("Ethernet")],
            &[route(1, unresolved_gateway), route(10, resolved_gateway)],
            &[
                neighbor(unresolved_gateway, NeighborState::Unusable),
                neighbor(resolved_gateway, NeighborState::Probe),
            ],
        );

        assert_eq!(selected[0].gateway_address, resolved_gateway);
    }

    #[test]
    fn selects_a_preferred_address_on_the_gateway_subnet() {
        let gateway = Ipv4Addr::new(10, 0, 0, 1);
        let mut source = adapter("Ethernet");
        source.addresses = vec![
            AdapterAddress {
                address: Ipv4Addr::new(192, 168, 1, 20),
                prefix_length: 24,
                is_preferred: true,
            },
            AdapterAddress {
                address: Ipv4Addr::new(10, 0, 0, 30),
                prefix_length: 24,
                is_preferred: false,
            },
            AdapterAddress {
                address: Ipv4Addr::new(10, 0, 0, 20),
                prefix_length: 24,
                is_preferred: true,
            },
        ];

        let selected = select_interfaces(
            &[source],
            &[route(10, gateway)],
            &[neighbor(gateway, NeighborState::Permanent)],
        );

        assert_eq!(selected[0].local_address, Ipv4Addr::new(10, 0, 0, 20));
    }

    #[test]
    fn correlates_by_luid_in_preference_to_a_reused_index() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        let mut wrong_route = route(1, gateway);
        wrong_route.luid = 99;

        let selected = select_interfaces(
            &[adapter("Ethernet")],
            &[wrong_route],
            &[neighbor(gateway, NeighborState::Reachable)],
        );

        assert!(selected.is_empty());
    }

    #[test]
    fn sorts_interfaces_case_insensitively_with_stable_ties() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        let mut wifi = adapter("Wi-Fi");
        wifi.luid = 20;
        wifi.index = 8;
        wifi.interface_type = InterfaceType::WiFi;
        let mut wifi_route = route(10, gateway);
        wifi_route.luid = 20;
        wifi_route.index = 8;
        let mut wifi_neighbor = neighbor(gateway, NeighborState::Reachable);
        wifi_neighbor.luid = 20;
        wifi_neighbor.index = 8;

        let selected = select_interfaces(
            &[wifi, adapter("ethernet")],
            &[wifi_route, route(10, gateway)],
            &[wifi_neighbor, neighbor(gateway, NeighborState::Reachable)],
        );

        assert_eq!(selected[0].friendly_name, "ethernet");
        assert_eq!(selected[1].friendly_name, "Wi-Fi");
    }

    #[test]
    fn rejects_invalid_guids_and_gateway_macs() {
        let gateway = Ipv4Addr::new(192, 168, 1, 1);
        let mut invalid_guid = adapter("Ethernet");
        invalid_guid.adapter_name = "not-a-guid".to_owned();
        let mut multicast_mac = neighbor(gateway, NeighborState::Reachable);
        multicast_mac.mac[0] = 1;

        assert!(
            select_interfaces(
                &[invalid_guid],
                &[route(10, gateway)],
                &[neighbor(gateway, NeighborState::Reachable)],
            )
            .is_empty()
        );
        assert!(
            select_interfaces(
                &[adapter("Ethernet")],
                &[route(10, gateway)],
                &[multicast_mac],
            )
            .is_empty()
        );
    }
}
