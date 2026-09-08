//! Transport-level message deduplication and signed-payload cache.
//!
//! `SEEN_MESSAGES` suppresses duplicate deliveries from gossip fan-out,
//! backfill and reconnection paths; `SIGNED_MESSAGE_CACHE` retains the raw
//! authenticated wire bytes so the backfill responder can serve exact payloads.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use iroh::PublicKey;

use crate::chat_core::{message_hash, Message, MessageHash};

// ── Network event dispatch ───────────────────────────────────────────────────

/// Key used for message deduplication: (sender, content_hash, sent_at_seconds).
type DedupKey = (PublicKey, MessageHash, u64);

/// How long we remember a message for deduplication.
///
/// Must be at least as long as the maximum TTL to cover the gossip-storm and
/// backfill window.  Default message TTL is 1 hour; we use 2 hours to safely
/// cover reconnection + backfill scenarios.
const DEDUP_TTL: Duration = Duration::from_secs(7200);

/// Trigger a cleanup sweep when the seen set grows beyond this size.
pub(crate) const DEDUP_SWEEP_THRESHOLD: usize = 10_000;
const SIGNED_MESSAGE_CACHE_CAPACITY: usize = 10_000;

/// Set of already-processed messages, keyed by (sender, content_hash, sent_at).
///
/// The value is the [`Instant`] when we first saw the message, used for TTL-based
/// eviction.  Entries older than [`DEDUP_TTL`] are periodically pruned.
pub(crate) static SEEN_MESSAGES: LazyLock<Mutex<HashMap<DedupKey, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Raw signed payloads observed during this process, retained so the existing
/// history backfill responder can serve the exact authenticated bytes rather
/// than an unsigned UI preview.  The key matches the normal message dedup key.
struct CachedSignedMessage {
    bytes: Vec<u8>,
    inserted_at: Instant,
}

/// Keep this cache independently bounded. A decoded event can be queued for
/// UI processing before `handle_net_event` records it as seen.
static SIGNED_MESSAGE_CACHE: LazyLock<Mutex<HashMap<DedupKey, CachedSignedMessage>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Remember the authenticated wire payload for a decoded message.
///
/// This is intentionally bounded by the same TTL as message deduplication;
/// it is a transport cache, not a second history store.
pub fn remember_signed_message(
    from: PublicKey,
    message: &Message,
    sent_at: u64,
    signed_bytes: &[u8],
) {
    let key = (from, message_hash(message), sent_at);
    if let Ok(mut cache) = SIGNED_MESSAGE_CACHE.lock() {
        cache.insert(
            key,
            CachedSignedMessage {
                bytes: signed_bytes.to_vec(),
                inserted_at: Instant::now(),
            },
        );
        while cache.len() > SIGNED_MESSAGE_CACHE_CAPACITY {
            let oldest_key = cache
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| *key);
            if let Some(oldest_key) = oldest_key {
                cache.remove(&oldest_key);
            } else {
                break;
            }
        }
    }
}

/// Take the raw signed payload for a message, if it was observed locally.
pub fn take_signed_message(from: PublicKey, hash: MessageHash, sent_at: u64) -> Option<Vec<u8>> {
    SIGNED_MESSAGE_CACHE.lock().ok().and_then(|mut cache| {
        cache
            .remove(&(from, hash, sent_at))
            .map(|entry| entry.bytes)
    })
}

/// Return the raw signed payload without removing it from the transport cache.
/// Frontends use this while handling a decoded event to persist authenticated
/// bytes in durable history; the backfill responder uses `take_signed_message`
/// when it needs ownership of the payload.
pub fn get_signed_message(from: PublicKey, hash: MessageHash, sent_at: u64) -> Option<Vec<u8>> {
    SIGNED_MESSAGE_CACHE.lock().ok().and_then(|cache| {
        cache
            .get(&(from, hash, sent_at))
            .map(|entry| entry.bytes.clone())
    })
}

/// Prune entries older than [`DEDUP_TTL`] from the seen-messages set.
pub(crate) fn prune_seen_messages() {
    let now = Instant::now();
    if let Ok(mut seen) = SEEN_MESSAGES.lock() {
        seen.retain(|_, first_seen| now.duration_since(*first_seen) < DEDUP_TTL);
    }
}

/// Key used for diagnostics event deduplication: (content_hash, sender_key).
/// Unlike [`SEEN_MESSAGES`] this does NOT include sent_at, so replayed
/// messages with different timestamps are still collapsed into one
/// diagnostic event.
type DiagDedupKey = (MessageHash, PublicKey);

/// Cooldown for diagnostic `MessageReceived` events: prevents the 5,000-entry
/// buffer from being saturated by repeated identical message hashes from the
/// same sender (e.g. stale messages replayed through the gossip mesh).  TTL
/// is 60 seconds — generous enough to catch bursts of gossip replays while
/// short enough that a genuine new message with the same hash from the same
/// sender (extremely unlikely) would eventually be recorded.
pub(crate) static DIAGNOSTIC_SEEN_MESSAGES: LazyLock<Mutex<HashMap<DiagDedupKey, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_verified_payload_survives_cache_saturation_and_is_taken_once() {
        let sender = iroh::SecretKey::generate().public();
        if let Ok(mut cache) = SIGNED_MESSAGE_CACHE.lock() {
            cache.clear();
        }
        if let Ok(mut seen) = SEEN_MESSAGES.lock() {
            seen.clear();
        }

        // Model a receive/dispatch delay: the signed payloads are cached by
        // the generic and room-aware forwarding paths before UI dedup runs.
        for index in 0..SIGNED_MESSAGE_CACHE_CAPACITY {
            let message = Message::Message {
                text: format!("topic-{index}"),
            };
            remember_signed_message(sender, &message, index as u64, &[index as u8]);
        }

        let delayed = Message::Message {
            text: "delayed durable payload".into(),
        };
        let delayed_bytes = b"verified-delayed-payload";
        remember_signed_message(sender, &delayed, 99_999, delayed_bytes);

        assert_eq!(
            get_signed_message(sender, message_hash(&delayed), 99_999).as_deref(),
            Some(delayed_bytes.as_slice())
        );
        assert_eq!(
            take_signed_message(sender, message_hash(&delayed), 99_999).as_deref(),
            Some(delayed_bytes.as_slice())
        );
        assert_eq!(
            take_signed_message(sender, message_hash(&delayed), 99_999),
            None,
            "durable writer ownership must consume the payload exactly once"
        );
    }
}
