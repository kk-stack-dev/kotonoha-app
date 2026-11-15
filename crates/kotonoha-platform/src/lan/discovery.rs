//! Finding computers that accept work, with mDNS (DNS-SD). mDNS packets stay on the local link.
//!
//! The first use is what makes macOS ask for the local network permission, so these run only after
//! the person turned a LAN feature on.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use kotonoha_core::lan::{address, protocol, SERVICE_TYPE};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

use super::LanError;

/// A computer that accepts work, as announced on the LAN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundComputer {
    /// Short id of its public key.
    pub id: String,
    pub name: String,
    pub addresses: Vec<SocketAddr>,
}

fn discovery_error(e: mdns_sd::Error) -> LanError {
    LanError::Discovery(e.to_string())
}

/// Listens for announcements for `wait`, then returns what was found (blocking). `own_id` is left out.
/// Only LAN addresses are kept; IPv6 link-local ones are left out because they need an interface.
pub fn browse(wait: Duration, own_id: Option<&str>) -> Result<Vec<FoundComputer>, LanError> {
    let daemon = ServiceDaemon::new().map_err(discovery_error)?;
    let events = daemon.browse(SERVICE_TYPE).map_err(discovery_error)?;
    let deadline = Instant::now() + wait;
    let mut found = BTreeMap::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else { break };
        if let ServiceEvent::ServiceResolved(service) = event {
            let Some(id) = service.get_property_val_str("id").map(str::to_owned) else { continue };
            if Some(id.as_str()) == own_id || id.is_empty() {
                continue;
            }
            let name = protocol::clean_name(service.get_property_val_str("name").unwrap_or_default());
            let addresses: Vec<SocketAddr> = service
                .get_addresses()
                .iter()
                .map(|ip| ip.to_ip_addr())
                .filter(|ip| address::is_lan(*ip) && !is_link_local_v6(*ip))
                .map(|ip| SocketAddr::new(ip, service.port))
                .collect();
            if addresses.is_empty() {
                continue;
            }
            let entry = found.entry(id.clone()).or_insert_with(|| FoundComputer { id, name, addresses: vec![] });
            for addr in addresses {
                if !entry.addresses.contains(&addr) {
                    entry.addresses.push(addr);
                }
            }
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    let mut list: Vec<FoundComputer> = found.into_values().collect();
    for computer in &mut list {
        // IPv4 first: the usual home network
        computer.addresses.sort_by_key(|a| (a.is_ipv6(), *a));
    }
    Ok(list)
}

fn is_link_local_v6(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)
}

/// Announces this computer while it accepts work. Dropping it withdraws the announcement.
pub struct Advertisement {
    daemon: ServiceDaemon,
    fullname: String,
}

pub fn advertise(name: &str, id: &str, port: u16) -> Result<Advertisement, LanError> {
    let daemon = ServiceDaemon::new().map_err(discovery_error)?;
    let version = protocol::VERSION.to_string();
    let properties = [("id", id), ("name", name), ("v", version.as_str())];
    let host = format!("kotonoha-{id}.local.");
    let service = ServiceInfo::new(SERVICE_TYPE, id, &host, "", port, &properties[..])
        .map_err(discovery_error)?
        .enable_addr_auto();
    let fullname = service.get_fullname().to_owned();
    daemon.register(service).map_err(discovery_error)?;
    tracing::info!(%id, port, "announcing on the LAN");
    Ok(Advertisement { daemon, fullname })
}

impl Drop for Advertisement {
    fn drop(&mut self) {
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            // Sends the goodbye packet so other computers drop this one from their lists at once
            let _ = done.recv_timeout(Duration::from_secs(1));
        }
        let _ = self.daemon.shutdown();
        tracing::info!("stopped announcing on the LAN");
    }
}
