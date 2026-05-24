use std::net::SocketAddr;

/// Resolve a config address string to a [`SocketAddr`] suitable for binding.
///
/// - `"example.com:443"` → `"0.0.0.0:443"` (bind to all interfaces)
/// - `"0.0.0.0:9090"` → `"0.0.0.0:9090"`
pub fn resolve_listen_addr(address: &str) -> anyhow::Result<SocketAddr> {
    if let Ok(addr) = address.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let port = address
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("Cannot parse address: {}", address))?;

    Ok(SocketAddr::from(([0, 0, 0, 0], port)))
}

/// Extract the hostname portion from an address string (used as SNI key).
///
/// - `"example.com:443"` → `"example.com"`
/// - `"[::1]:443"` → `"::1"`
/// - `"0.0.0.0:9090"` → `"0.0.0.0"`
pub fn extract_hostname(address: &str) -> &str {
    if address.starts_with('[') {
        if let Some(end) = address.find(']') {
            return &address[1..end];
        }
    }
    address.rsplit(':').next_back().unwrap_or(address)
}

/// HTTP port for redirect listeners derived from the TLS listen port.
///
/// Formula: `tls_port - 443 + 80` (443→80, 8443→8080, etc.)
pub fn tls_redirect_port(tls_port: u16) -> u16 {
    tls_port.saturating_sub(443).saturating_add(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_listen_addr_ip_port() {
        let addr = resolve_listen_addr("0.0.0.0:8443").unwrap();
        assert_eq!(addr, "0.0.0.0:8443".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn test_resolve_listen_addr_hostname_port() {
        let addr = resolve_listen_addr("example.com:443").unwrap();
        assert_eq!(addr.port(), 443);
        assert_eq!(
            addr.ip(),
            std::net::IpAddr::from(std::net::Ipv4Addr::new(0, 0, 0, 0))
        );
    }

    #[test]
    fn test_resolve_listen_addr_localhost() {
        let addr = resolve_listen_addr("localhost:8080").unwrap();
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_resolve_listen_addr_invalid() {
        assert!(resolve_listen_addr("no-port-here").is_err());
    }

    #[test]
    fn test_extract_hostname_ipv4() {
        assert_eq!(extract_hostname("example.com:443"), "example.com");
    }

    #[test]
    fn test_extract_hostname_ipv6() {
        assert_eq!(extract_hostname("[::1]:443"), "::1");
    }

    #[test]
    fn test_extract_hostname_ip() {
        assert_eq!(extract_hostname("0.0.0.0:9090"), "0.0.0.0");
    }

    #[test]
    fn test_extract_hostname_localhost() {
        assert_eq!(extract_hostname("localhost:8080"), "localhost");
    }

    #[test]
    fn test_tls_redirect_port() {
        assert_eq!(tls_redirect_port(443), 80);
        assert_eq!(tls_redirect_port(8443), 8080);
        assert_eq!(tls_redirect_port(44300), 43937);
    }
}
