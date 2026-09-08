//! Local-only Home network information. No hosted IP lookup requests.
use iroh::{Endpoint, EndpointAddr, TransportAddr, Watcher};
use maxminddb::Reader;
use n0_future::StreamExt;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
/// Display-only local network details; never broadcast to peers.
pub struct Snapshot {
    /// Public and local addresses advertised by the local Iroh endpoint.
    pub addresses: String,
    /// Approximate IP-based location or an explicit unavailable message.
    pub location: String,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            addresses: "IP: discovering…".into(),
            location: "Approximate location: loading local database…".into(),
        }
    }
}

#[derive(Deserialize)]
struct Place {
    names: Option<BTreeMap<String, String>>,
}
#[derive(Deserialize)]
struct Record {
    city: Option<Place>,
    subdivisions: Option<Vec<Place>>,
    country: Option<Place>,
}
fn english(place: Option<Place>) -> Option<String> {
    place?.names?.remove("en")
}
fn bundled_reader() -> Result<Reader<Vec<u8>>, String> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&include_bytes!("../assets/geoip/dbip-city-lite.mmdb.gz")[..])
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Reader::from_source(bytes).map_err(|error| error.to_string())
}
fn snapshot(address: &EndpointAddr, reader: Option<&Reader<Vec<u8>>>) -> Snapshot {
    let mut public = Vec::new();
    let mut local = Vec::new();
    for transport in &address.addrs {
        if let TransportAddr::Ip(addr) = transport {
            let ip = addr.ip();
            if crate::network_location::is_public_ip(ip) {
                public.push(ip);
            } else {
                local.push(ip);
            }
        }
    }
    public.sort();
    public.dedup();
    local.sort();
    local.dedup();
    let display = |ips: &[std::net::IpAddr]| {
        ips.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let addresses = format!(
        "Public IP: {}\nLocal IP: {}",
        if public.is_empty() {
            "not available".into()
        } else {
            display(&public)
        },
        if local.is_empty() {
            "not available".into()
        } else {
            display(&local)
        }
    );
    let location = public
        .iter()
        .find_map(|ip| {
            let record = reader?.lookup(*ip).ok()?.decode::<Record>().ok()??;
            let mut parts = Vec::new();
            if let Some(city) = english(record.city) {
                parts.push(city);
            }
            if let Some(regions) = record.subdivisions {
                for region in regions {
                    if let Some(name) = english(Some(region)) {
                        parts.push(name);
                    }
                }
            }
            if let Some(country) = english(record.country) {
                parts.push(country);
            }
            (!parts.is_empty())
                .then(|| format!("Approximate location ({ip}): {}", parts.join(", ")))
        })
        .unwrap_or_else(|| {
            if public.is_empty() {
                "Approximate location: unavailable until a public IP is discovered".into()
            } else {
                "Approximate location: unavailable in the local database".into()
            }
        });
    Snapshot {
        addresses,
        location,
    }
}

/// Watch endpoint changes and resolve local-only geography off the GUI thread.
pub fn start(runtime: &tokio::runtime::Handle, endpoint: Endpoint) -> Arc<Mutex<Snapshot>> {
    let state = Arc::new(Mutex::new(Snapshot::default()));
    let output = Arc::clone(&state);
    runtime.spawn(async move {
        // Decompression and file IO must not block the GUI or networking workers.
        let reader = tokio::task::spawn_blocking(|| {
            if let Some(path) = std::env::var_os("BORU_GEOIP_CITY") {
                if let Ok(reader) = Reader::open_readfile(path) {
                    return Ok(reader);
                }
                tracing::warn!("Configured GeoIP database unavailable; using bundled DB-IP Lite");
            }
            bundled_reader()
        })
        .await
        .ok()
        .and_then(|result| {
            result
                .map_err(|error| tracing::warn!(%error, "Local GeoIP unavailable"))
                .ok()
        });
        let mut addresses = endpoint.watch_addr().stream();
        while let Some(address) = addresses.next().await {
            if let Ok(mut value) = output.lock() {
                *value = snapshot(&address, reader.as_ref());
            }
        }
    });
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_database_resolves_public_ip_offline() {
        let reader = bundled_reader().expect("bundled database must decode");
        let record = reader
            .lookup("8.8.8.8".parse().unwrap())
            .unwrap()
            .decode::<Record>()
            .unwrap()
            .unwrap();
        assert!(english(record.country).is_some());
    }

    fn address(ips: &[&str]) -> EndpointAddr {
        let mut address = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7; 32]).public());
        for ip in ips {
            address.addrs.insert(TransportAddr::Ip(ip.parse().unwrap()));
        }
        address
    }

    #[test]
    fn private_ip_is_not_geolocated() {
        let state = snapshot(&address(&["192.168.1.20:1234"]), None);
        assert!(state.addresses.contains("Public IP: not available"));
        assert!(state.addresses.contains("Local IP: 192.168.1.20"));
        assert!(state.location.contains("until a public IP is discovered"));
    }

    #[test]
    fn location_resets_when_public_address_disappears() {
        let reader = bundled_reader().unwrap();
        let public = snapshot(&address(&["8.8.8.8:1234"]), Some(&reader));
        assert!(public.location.contains("Approximate location (8.8.8.8):"));
        let gone = snapshot(&address(&[]), Some(&reader));
        assert!(!gone.location.contains("8.8.8.8"));
        assert!(gone.location.contains("unavailable"));
    }

    #[test]
    fn missing_database_keeps_addresses_and_reports_unavailable() {
        let state = snapshot(
            &address(&[
                "8.8.8.8:1234",
                "8.8.8.8:5678",
                "[2001:4860:4860::8888]:1234",
            ]),
            None,
        );
        assert_eq!(state.addresses.matches("8.8.8.8").count(), 1);
        assert!(state.addresses.contains("2001:4860:4860::8888"));
        assert!(state.location.contains("unavailable in the local database"));
    }
}
