//! Passive Quick Share discovery over the computer's existing LAN.
//!
//! Quick Share listeners publish a DNS-SD record with a dynamic TCP port, so
//! browsing mDNS is both faster and less intrusive than sweeping port ranges.

use crate::quick_share::{QuickShareDeviceType, QUICK_SHARE_SERVICE};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use mdns_sd::{Receiver, ScopedIp, ServiceDaemon, ServiceEvent};
use std::net::{SocketAddr, SocketAddrV6};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NearbyQuickShareDevice {
    pub id: String,
    pub name: String,
    pub device_type: QuickShareDeviceType,
    pub addresses: Vec<SocketAddr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickShareDiscoveryEvent {
    Found(NearbyQuickShareDevice),
    Removed(String),
}

pub struct QuickShareBrowser {
    daemon: ServiceDaemon,
    events: Receiver<ServiceEvent>,
}

impl QuickShareBrowser {
    pub fn start() -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        let events = daemon.browse(QUICK_SHARE_SERVICE)?;
        Ok(Self { daemon, events })
    }

    pub fn poll(&self) -> Vec<QuickShareDiscoveryEvent> {
        let mut updates = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                ServiceEvent::ServiceResolved(service) => {
                    if let Some(device) = device_from_service(&service) {
                        updates.push(QuickShareDiscoveryEvent::Found(device));
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    updates.push(QuickShareDiscoveryEvent::Removed(fullname));
                }
                _ => {}
            }
        }
        updates
    }
}

impl Drop for QuickShareBrowser {
    fn drop(&mut self) {
        let _ = self.daemon.stop_browse(QUICK_SHARE_SERVICE);
        let _ = self.daemon.shutdown();
    }
}

fn device_from_service(service: &mdns_sd::ResolvedService) -> Option<NearbyQuickShareDevice> {
    let (name, device_type) = decode_visible_endpoint_info(service.get_property_val_str("n")?)?;
    let mut addresses = service
        .get_addresses()
        .iter()
        .filter_map(|address| match address {
            ScopedIp::V4(ip) => Some(SocketAddr::new((*ip.addr()).into(), service.get_port())),
            ScopedIp::V6(ip) => Some(SocketAddr::V6(SocketAddrV6::new(
                *ip.addr(),
                service.get_port(),
                0,
                ip.scope_id().index,
            ))),
            _ => None,
        })
        .collect::<Vec<_>>();
    addresses.sort_by_key(SocketAddr::is_ipv6);
    (!addresses.is_empty()).then(|| NearbyQuickShareDevice {
        id: service.get_fullname().to_owned(),
        name,
        device_type,
        addresses,
    })
}

fn decode_visible_endpoint_info(encoded: &str) -> Option<(String, QuickShareDeviceType)> {
    let info = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    if info.len() < 18 || info[0] & 0x10 != 0 {
        return None;
    }
    let device_type = match (info[0] >> 1) & 0x07 {
        1 => QuickShareDeviceType::Phone,
        2 => QuickShareDeviceType::Tablet,
        3 => QuickShareDeviceType::Laptop,
        _ => QuickShareDeviceType::Unknown,
    };
    let length = info[17] as usize;
    let end = 18_usize
        .checked_add(length)
        .filter(|end| *end <= info.len())?;
    let name = std::str::from_utf8(&info[18..end]).ok()?.to_owned();
    Some((name, device_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QuickShareAdvertisementData, QuickShareEndpointId};

    #[test]
    fn decodes_a_visible_windows_or_mac_laptop() {
        let advertisement = QuickShareAdvertisementData::from_parts(
            QuickShareEndpointId::new(*b"W1n9").unwrap(),
            [0x55; 16],
            QuickShareDeviceType::Laptop,
            "OFFICE-LAPTOP",
        )
        .unwrap();
        assert_eq!(
            decode_visible_endpoint_info(&advertisement.endpoint_info),
            Some(("OFFICE-LAPTOP".into(), QuickShareDeviceType::Laptop))
        );
    }

    #[test]
    fn ignores_hidden_non_qr_advertisements() {
        let encoded = URL_SAFE_NO_PAD.encode([0x10; 18]);
        assert_eq!(decode_visible_endpoint_info(&encoded), None);
    }
}
