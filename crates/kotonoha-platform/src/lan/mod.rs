//! Processing on another computer in the same LAN: encrypted connections, the computer that accepts
//! work, the one that sends it, discovery with mDNS, and the stored keys of paired computers.
//!
//! Nothing here runs until the app turns a LAN feature on: no socket is opened, no key is created and
//! nothing is announced before that.

mod client;
mod discovery;
mod error;
mod identity;
pub mod network;
mod peers;
mod server;
mod transport;

pub use client::{pair, LanClient};
pub use discovery::{advertise, browse, Advertisement, FoundComputer};
pub use error::LanError;
pub use identity::{Identity, IdentitySource};
pub use peers::{PairedClient, PairedServer, PeerStore, Peers};
pub use server::{start, ChosenModels, LocalEngines, LocalWork, PairingApprover, PairingRequest, Server, ServerConfig};

/// Random bytes from the OS.
pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut out = [0; N];
    getrandom::fill(&mut out).expect("the OS random number generator is available");
    out
}

/// This computer's name as others see it ("Kotonoha-MacBook" rather than "Kotonoha-MacBook.local").
pub fn computer_name() -> String {
    let name = sysinfo::System::host_name().unwrap_or_default();
    let name = name.strip_suffix(".local").unwrap_or(&name);
    kotonoha_core::lan::protocol::clean_name(name)
}

/// This computer's IPv4 addresses inside the LAN, for typing on another computer when discovery
/// does not work.
pub fn local_addresses() -> Vec<std::net::IpAddr> {
    let mut list: Vec<std::net::IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.ip())
        .filter(|ip| ip.is_ipv4() && !ip.is_loopback() && kotonoha_core::lan::address::is_lan(*ip))
        .collect();
    list.sort();
    list.dedup();
    list
}
