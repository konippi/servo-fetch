//! Network address validation — blocks private, reserved, and special-purpose IPs.

/// Returns `true` if the host resolves to a private or reserved address.
pub(crate) fn is_private_host(host: &str) -> bool {
    // Fast path for common blocked hosts; comprehensive check follows via IP parsing.
    const BLOCKED_HOSTS: &[&str] = &[
        "localhost",
        "127.0.0.1",
        "[::1]",
        "0.0.0.0",
        "169.254.169.254",
        "metadata.google.internal",
    ];
    if BLOCKED_HOSTS.iter().any(|&b| host.eq_ignore_ascii_case(b)) {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return is_private_ipv4(ip);
    }
    if let Ok(ip) = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<std::net::Ipv6Addr>()
    {
        return is_private_ipv6(&ip);
    }
    false
}

/// IANA IPv4 Special-Purpose Address Registry (RFC 6890) + cloud metadata.
fn is_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    const BLOCKED: &[(u32, u32)] = &[
        (0x0000_0000, 0xff00_0000), // 0.0.0.0/8        — RFC 1122 current network
        (0x0a00_0000, 0xff00_0000), // 10.0.0.0/8       — RFC 1918 private
        (0x6440_0000, 0xffc0_0000), // 100.64.0.0/10    — RFC 6598 shared/CGN
        (0x7f00_0000, 0xff00_0000), // 127.0.0.0/8      — RFC 1122 loopback
        (0xa9fe_0000, 0xffff_0000), // 169.254.0.0/16   — RFC 3927 link-local
        (0xac10_0000, 0xfff0_0000), // 172.16.0.0/12    — RFC 1918 private
        (0xc000_0000, 0xffff_ff00), // 192.0.0.0/24     — RFC 6890 IETF protocol
        (0xc000_0200, 0xffff_ff00), // 192.0.2.0/24     — RFC 5737 TEST-NET-1
        (0xc058_6300, 0xffff_ff00), // 192.88.99.0/24   — RFC 3068 6to4 relay
        (0xc0a8_0000, 0xffff_0000), // 192.168.0.0/16   — RFC 1918 private
        (0xc612_0000, 0xfffe_0000), // 198.18.0.0/15    — RFC 2544 benchmarking
        (0xc633_6400, 0xffff_ff00), // 198.51.100.0/24  — RFC 5737 TEST-NET-2
        (0xcb00_7100, 0xffff_ff00), // 203.0.113.0/24   — RFC 5737 TEST-NET-3
        (0xe000_0000, 0xf000_0000), // 224.0.0.0/4      — RFC 5771 multicast
        (0xf000_0000, 0xf000_0000), // 240.0.0.0/4      — RFC 1112 reserved
        (0xffff_ffff, 0xffff_ffff), // 255.255.255.255  — broadcast
    ];
    let bits = u32::from(ip);
    BLOCKED.iter().any(|&(net, mask)| bits & mask == net)
}

/// IANA IPv6 Special-Purpose Address Registry + IPv4-mapped/compatible.
fn is_private_ipv6(ip: &std::net::Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()) {
        return is_private_ipv4(v4);
    }
    let seg = ip.segments();
    let s0 = seg[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || (s0 == 0x0100 && seg[1] == 0 && seg[2] == 0 && seg[3] == 0) // 0100::/64 discard (RFC 6666)
        || (s0 == 0x2001 && seg[1] == 0)                               // 2001::/32 Teredo
        || (s0 == 0x2001 && seg[1] & 0xfff0 == 0x0010)                 // 2001:10::/28 ORCHID
        || (s0 == 0x2001 && seg[1] & 0xfff0 == 0x0020)                 // 2001:20::/28 ORCHIDv2
        || (s0 == 0x2001 && seg[1] == 0x0db8)                          // 2001:db8::/32 documentation
        || s0 == 0x2002                                                // 2002::/16 6to4
        || s0 & 0xfe00 == 0xfc00                                       // fc00::/7  unique local
        || s0 & 0xffc0 == 0xfe80                                       // fe80::/10 link-local
        || s0 & 0xff00 == 0xff00 // ff00::/8  multicast
}

/// Validate and sanitize a URL for fetching.
///
/// Only `http`/`https` schemes are accepted, embedded credentials are stripped,
/// and hosts that resolve to private or reserved addresses are rejected. DNS is
/// not resolved, so DNS rebinding attacks remain possible downstream.
pub(crate) fn validate_url(input: &str) -> anyhow::Result<url::Url> {
    use anyhow::{Context as _, bail};

    let mut parsed = url::Url::parse(input).with_context(|| format!("invalid URL: {input}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        s => bail!("scheme '{s}' not allowed; only http:// and https:// are supported"),
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        eprintln!("warning: credentials stripped from URL");
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
    }
    if let Some(host) = parsed.host_str() {
        if is_private_host(host) {
            bail!("access to private/local addresses is not allowed: {host}");
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("[::1]"));
    }

    #[test]
    fn blocks_private_ipv4() {
        assert!(is_private_host("10.0.0.1"));
        assert!(is_private_host("192.168.1.1"));
        assert!(is_private_host("172.16.0.1"));
    }

    #[test]
    fn blocks_shared_cgn() {
        assert!(is_private_host("100.64.0.1"));
        assert!(is_private_host("100.127.255.254"));
    }

    #[test]
    fn blocks_documentation_ips() {
        assert!(is_private_host("192.0.2.1"));
        assert!(is_private_host("198.51.100.1"));
        assert!(is_private_host("203.0.113.1"));
    }

    #[test]
    fn blocks_multicast() {
        assert!(is_private_host("224.0.0.1"));
    }

    #[test]
    fn blocks_metadata() {
        assert!(is_private_host("169.254.169.254"));
        assert!(is_private_host("metadata.google.internal"));
    }

    #[test]
    fn blocks_ipv4_mapped_ipv6() {
        assert!(is_private_host("::ffff:127.0.0.1"));
        assert!(is_private_host("::ffff:10.0.0.1"));
    }

    #[test]
    fn blocks_ipv6_special() {
        assert!(is_private_host("fe80::1"));
        assert!(is_private_host("fd00::1"));
        assert!(is_private_host("2001:db8::1"));
    }

    #[test]
    fn allows_public() {
        assert!(!is_private_host("8.8.8.8"));
        assert!(!is_private_host("1.1.1.1"));
        assert!(!is_private_host("example.com"));
    }

    #[test]
    fn blocks_zero_address() {
        assert!(is_private_host("0.0.0.0"));
    }

    #[test]
    fn blocks_localhost_case_insensitive() {
        assert!(is_private_host("LOCALHOST"));
        assert!(is_private_host("Localhost"));
    }

    #[test]
    fn blocks_broadcast() {
        assert!(is_private_host("255.255.255.255"));
    }

    #[test]
    fn blocks_reserved_240() {
        assert!(is_private_host("240.0.0.1"));
    }

    #[test]
    fn blocks_teredo() {
        assert!(is_private_host("2001::1"));
    }

    #[test]
    fn blocks_6to4() {
        assert!(is_private_host("2002::1"));
    }

    #[test]
    fn validate_url_empty_string() {
        assert!(validate_url("").is_err());
    }
}
