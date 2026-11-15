//! Which addresses count as "this LAN". Audio is only ever sent to, and accepted from, these.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use super::DEFAULT_PORT;

/// Private IPv4 ranges, link-local and loopback, and their IPv6 counterparts (unique local
/// fc00::/7, link-local fe80::/10, ::1). Everything else, including carrier-grade NAT
/// (100.64.0.0/10) and public addresses, is outside the LAN.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_lan_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_lan_v4(v4),
            None => is_lan_v6(v6),
        },
    }
}

fn is_lan_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local() || ip.is_loopback()
}

fn is_lan_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("not an IP address")]
    Invalid,
    #[error("the address is outside this LAN")]
    NotLan,
}

/// Parses an address typed by the user: "192.168.1.20", "192.168.1.20:50540", "fe80::1" or
/// "[fe80::1]:50540". Host names are not accepted, so nothing is looked up through DNS.
pub fn parse(text: &str) -> Result<SocketAddr, AddressError> {
    let text = text.trim();
    let addr = text
        .parse::<SocketAddr>()
        .or_else(|_| text.parse::<IpAddr>().map(|ip| SocketAddr::new(ip, DEFAULT_PORT)))
        .map_err(|_| AddressError::Invalid)?;
    if addr.port() == 0 {
        return Err(AddressError::Invalid);
    }
    if !is_lan(addr.ip()) {
        return Err(AddressError::NotLan);
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_link_local_and_loopback_are_lan() {
        for s in ["10.0.0.5", "172.16.0.1", "172.31.255.254", "192.168.1.20", "169.254.3.4", "127.0.0.1"] {
            assert!(is_lan(ip(s)), "{s}");
        }
        for s in ["::1", "fe80::1", "fd12:3456::1", "fc00::1", "::ffff:192.168.0.2"] {
            assert!(is_lan(ip(s)), "{s}");
        }
    }

    #[test]
    fn public_and_carrier_nat_are_not() {
        for s in ["8.8.8.8", "172.32.0.1", "100.64.0.1", "192.169.0.1", "0.0.0.0", "255.255.255.255"] {
            assert!(!is_lan(ip(s)), "{s}");
        }
        for s in ["2001:db8::1", "2606:4700::1111", "::ffff:8.8.8.8", "ff02::fb"] {
            assert!(!is_lan(ip(s)), "{s}");
        }
    }

    #[test]
    fn parses_typed_addresses() {
        assert_eq!(parse(" 192.168.1.20 ").unwrap(), "192.168.1.20:50540".parse().unwrap());
        assert_eq!(parse("192.168.1.20:6000").unwrap(), "192.168.1.20:6000".parse().unwrap());
        assert_eq!(parse("fe80::1").unwrap(), "[fe80::1]:50540".parse().unwrap());
        assert_eq!(parse("[fd00::2]:7000").unwrap(), "[fd00::2]:7000".parse().unwrap());
        assert_eq!(parse("8.8.8.8"), Err(AddressError::NotLan));
        assert_eq!(parse("example.com"), Err(AddressError::Invalid));
        assert_eq!(parse("my-mac.local"), Err(AddressError::Invalid));
        assert_eq!(parse("192.168.1.20:0"), Err(AddressError::Invalid));
        assert_eq!(parse(""), Err(AddressError::Invalid));
    }
}
