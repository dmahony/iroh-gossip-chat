//! Bounded HTTPS address discovery. No identity, cookies or chat data are sent.
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

const V4: &[&str] = &[
    "https://api.ipify.org",
    "https://checkip.amazonaws.com",
    "https://ipv4.icanhazip.com",
];
const V6: &[&str] = &["https://api6.ipify.org", "https://ipv6.icanhazip.com"];

fn parse(bytes: &[u8], ipv6: bool) -> Option<IpAddr> {
    if bytes.len() > 64 {
        return None;
    }
    let ip: IpAddr = std::str::from_utf8(bytes).ok()?.trim().parse().ok()?;
    let global_unicast = match ip {
        IpAddr::V4(ip) => ip.octets()[0] != 0 && ip.octets()[0] < 224,
        IpAddr::V6(ip) => {
            (ip.segments()[0] & 0xe000) == 0x2000
                && !(ip.segments()[0] == 0x3fff && ip.segments()[1] < 0x1000)
        }
    };
    (global_unicast && ip.is_ipv6() == ipv6 && crate::network_location::is_public_ip(ip))
        .then_some(ip)
}

async fn lookup(client: &reqwest::Client, urls: &[&str], ipv6: bool) -> Option<IpAddr> {
    for url in urls {
        let result = async {
            let mut response = client
                .get(*url)
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.ok()? {
                if bytes.len() + chunk.len() > 64 {
                    return None;
                }
                bytes.extend_from_slice(&chunk);
            }
            parse(&bytes, ipv6)
        };
        if let Ok(Some(ip)) = tokio::time::timeout(Duration::from_secs(4), result).await {
            return Some(ip);
        }
    }
    None
}

pub(super) async fn discover(ipv6: bool) -> Option<IpAddr> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        // Use OS routing (including system VPN routes), like Iroh.
        // No application/system HTTP proxy is used for this transport address check.
        .no_proxy()
        .local_address(if ipv6 {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        })
        .build()
        .ok()?;
    lookup(&client, if ipv6 { V6 } else { V4 }, ipv6).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_private_and_wrong_family() {
        for input in [
            "<html>error</html>",
            "127.0.0.1",
            "192.168.1.1",
            "100.64.1.1",
            "::1",
            "8.8.8.8 extra",
            "",
        ] {
            assert_eq!(parse(input.as_bytes(), false), None);
        }
        assert_eq!(parse(&[b'1'; 65], false), None);
        assert!(parse(b"8.8.8.8\n", false).is_some());
        assert!(parse(b"2001:4860:4860::8888\n", true).is_some());
        assert!(parse(b"8.8.8.8", true).is_none());
    }

    #[tokio::test]
    async fn falls_back_after_invalid_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for body in ["192.168.1.1", "8.8.8.8"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                stream.read(&mut request).await.unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert_eq!(
            lookup(&client, &[&url, &url], false).await,
            Some("8.8.8.8".parse().unwrap())
        );
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires Internet access; explicit live verification"]
    async fn live_ipv4_lookup() {
        let ip = discover(false).await.expect("public IPv4 lookup failed");
        println!("HTTPS public IPv4: {ip}");
    }
}
