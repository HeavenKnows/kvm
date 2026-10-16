use std::net::{IpAddr, Ipv4Addr};

pub fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut ips = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if useful_ipv4(ip) => Some(ip),
            _ => None,
        })
        .collect::<Vec<_>>();
    ips.sort_unstable();
    ips.dedup();
    ips
}

fn useful_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipv4_filter() {
        assert!(!useful_ipv4(Ipv4Addr::LOCALHOST));
        assert!(!useful_ipv4(Ipv4Addr::new(169,254,1,3)));
        assert!(useful_ipv4(Ipv4Addr::new(192,168,1,9)));
    }
}
