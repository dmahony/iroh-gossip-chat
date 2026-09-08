//! Network-level home status. Never depends on the selected chat's sender.

pub(super) fn network_connected(relay_connected: bool, has_peer_connections: bool) -> bool {
    relay_connected || has_peer_connections
}

pub(super) fn transport_label(relay_connected: bool, direct: u32, relayed: u32) -> &'static str {
    if direct > 0 && relayed > 0 {
        "Direct + relayed P2P"
    } else if direct > 0 {
        "Direct P2P"
    } else if relayed > 0 {
        "Relayed P2P"
    } else if relay_connected {
        "Relay connected · waiting for peers"
    } else {
        "Waiting for connection"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_is_connected_without_an_open_chat_or_peer() {
        assert!(network_connected(true, false));
        assert_eq!(
            transport_label(true, 0, 0),
            "Relay connected · waiting for peers"
        );
    }

    #[test]
    fn disconnect_does_not_stick_and_direct_only_connections_work() {
        assert!(!network_connected(false, false));
        assert!(network_connected(false, true));
    }

    #[test]
    fn transport_labels_do_not_claim_direct_connectivity_for_relay() {
        assert_eq!(transport_label(true, 0, 1), "Relayed P2P");
        assert_eq!(transport_label(false, 1, 0), "Direct P2P");
        assert_eq!(transport_label(true, 1, 1), "Direct + relayed P2P");
        assert_eq!(transport_label(false, 0, 0), "Waiting for connection");
    }
}
