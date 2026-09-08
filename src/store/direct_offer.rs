//! Durable direct-offer announcements and idempotent ticket upgrades.
use super::*;
use crate::chat_core::protocol::FileOfferId;
use crate::chat_core::{Message, SignedMessage};

/// Local projection; signed network payloads remain unchanged.
pub struct DirectOfferState {
    pub ready: Option<Vec<u8>>,
    pub local_path: Option<String>,
}

impl MessageStore {
    /// Persist before publishing or queueing an offer. Only the announcement
    /// creates a timeline row/unread; ready and poster updates amend its state.
    pub fn persist_direct_offer(
        &self,
        topic: &[u8; 32],
        signed: &[u8],
        local_user: &[u8; 32],
        local_path: Option<&str>,
    ) -> Result<()> {
        let (owner, message, sent_at) = SignedMessage::verify_and_decode(signed)?;
        let (offer_id, name, ready, has_thumbnail) = match message {
            Message::FileOffer { offer_id, name, .. } => (offer_id, Some(name), false, false),
            Message::FileOfferReady {
                offer_id,
                ticket,
                thumbnail_hash,
            } => {
                ticket
                    .parse::<iroh_blobs::ticket::BlobTicket>()
                    .std_context("invalid persisted offer ticket")?;
                (offer_id, None, true, thumbnail_hash.is_some())
            }
            _ => return Err(n0_error::anyerr!("not a direct-offer event")),
        };
        if crate::discovery_topic::is_discovery_topic(crate::proto::TopicId::from_bytes(*topic)) {
            return Err(n0_error::anyerr!(
                "cannot persist an offer on the discovery topic"
            ));
        }
        if local_path.is_some() && owner.as_bytes() != local_user {
            return Err(n0_error::anyerr!("remote offers cannot supply local paths"));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().std_context("begin direct offer")?;
        tx.execute(
            "INSERT INTO direct_offer_state(topic,owner,offer_id,local_path)
             VALUES(?1,?2,?3,?4) ON CONFLICT(topic,owner,offer_id)
             DO UPDATE SET local_path=COALESCE(excluded.local_path,direct_offer_state.local_path)",
            params![
                topic.as_slice(),
                owner.as_bytes().as_slice(),
                offer_id.as_bytes().as_slice(),
                local_path
            ],
        )
        .std_context("ensure direct offer state")?;
        if ready {
            // Posterless retries must not erase a poster. Older updates must
            // not replace newer tickets. Equal-second poster upgrades are valid.
            tx.execute(
                "UPDATE direct_offer_state SET ready_signed=?4, ready_at=?5, has_thumbnail=?6
                 WHERE topic=?1 AND owner=?2 AND offer_id=?3
                 AND (ready_signed IS NULL OR (ready_at<=?5 AND has_thumbnail<=?6))",
                params![
                    topic.as_slice(),
                    owner.as_bytes().as_slice(),
                    offer_id.as_bytes().as_slice(),
                    signed,
                    sent_at as i64,
                    has_thumbnail
                ],
            )
            .std_context("persist direct offer ready")?;
        } else if let Some(name) = name {
            let existing: Option<Vec<u8>> = tx.query_row(
                "SELECT announcement_hash FROM direct_offer_state WHERE topic=?1 AND owner=?2 AND offer_id=?3",
                params![topic.as_slice(), owner.as_bytes().as_slice(), offer_id.as_bytes().as_slice()],
                |r| r.get(0),
            ).std_context("read direct offer identity")?;
            if existing.is_none() {
                let hash = blake3::hash(signed);
                tx.execute(
                    "INSERT INTO messages(msg_hash,topic,sender,timestamp_ms,kind,body,signed_bytes,delivery_state)
                     VALUES(?1,?2,?3,?4,'file',?5,?6,'queued') ON CONFLICT(msg_hash) DO NOTHING",
                    params![hash.as_bytes().as_slice(),topic.as_slice(),owner.as_bytes().as_slice(),sent_at.saturating_mul(1000) as i64,name,signed],
                ).std_context("insert direct offer announcement")?;
                tx.execute(
                    "UPDATE direct_offer_state SET announcement_hash=?4 WHERE topic=?1 AND owner=?2 AND offer_id=?3",
                    params![topic.as_slice(),owner.as_bytes().as_slice(),offer_id.as_bytes().as_slice(),hash.as_bytes().as_slice()],
                ).std_context("link direct offer announcement")?;
                tx.execute(
                    "INSERT INTO conversation_meta(conversation_id,last_message_id,last_activity_at_ms,last_message_preview,last_author_user_id,unread_count)
                     VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(conversation_id) DO UPDATE SET
                     last_message_id=excluded.last_message_id,last_activity_at_ms=excluded.last_activity_at_ms,
                     last_message_preview=excluded.last_message_preview,last_author_user_id=excluded.last_author_user_id,
                     unread_count=conversation_meta.unread_count+excluded.unread_count",
                    params![topic.as_slice(),hash.as_bytes().as_slice(),sent_at.saturating_mul(1000) as i64,name,owner.as_bytes().as_slice(),i32::from(owner.as_bytes()!=local_user)],
                ).std_context("update direct offer conversation")?;
            }
        }
        tx.commit().std_context("commit direct offer")?;
        Ok(())
    }

    /// Mark the announcement published after gossip accepts the broadcast.
    /// Failed broadcasts leave it queued for durable retry handling.
    pub fn mark_direct_offer_sent(
        &self,
        topic: &[u8; 32],
        owner: &[u8; 32],
        offer_id: FileOfferId,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn
            .execute(
                "UPDATE messages SET delivery_state='sent'
                 WHERE msg_hash=(SELECT announcement_hash FROM direct_offer_state
                                 WHERE topic=?1 AND owner=?2 AND offer_id=?3)",
                params![topic.as_slice(), owner.as_slice(), offer_id.as_bytes().as_slice()],
            )
            .std_context("mark direct offer sent")?;
        if changed == 0 {
            return Err(n0_error::anyerr!("direct offer announcement not found"));
        }
        Ok(())
    }

    pub fn direct_offer_state(
        &self,
        topic: &[u8; 32],
        owner: &[u8; 32],
        offer_id: FileOfferId,
    ) -> Result<Option<DirectOfferState>> {
        self.conn.lock().unwrap().query_row(
            "SELECT ready_signed,local_path FROM direct_offer_state WHERE topic=?1 AND owner=?2 AND offer_id=?3",
            params![topic.as_slice(),owner.as_slice(),offer_id.as_bytes().as_slice()],
            |r| Ok(DirectOfferState { ready: r.get(0)?, local_path: r.get(1)? }),
        ).optional().std_context("read direct offer state")
    }

    /// Called only after a local download has completed successfully.
    pub fn set_direct_offer_local_path(
        &self,
        topic: &[u8; 32],
        owner: &[u8; 32],
        offer_id: FileOfferId,
        path: &Path,
    ) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "UPDATE direct_offer_state SET local_path=?4 WHERE topic=?1 AND owner=?2 AND offer_id=?3",
            params![topic.as_slice(),owner.as_slice(),offer_id.as_bytes().as_slice(),path.to_string_lossy()],
        ).std_context("save direct offer download path")?;
        if changed == 0 {
            return Err(n0_error::anyerr!("download has no persisted direct offer"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_offer_history_roundtrip_and_reordered_upgrades() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.db");
        let key = iroh::SecretKey::generate();
        let owner = *key.public().as_bytes();
        let topic = [7u8; 32];
        let id = FileOfferId::generate();
        let announcement = SignedMessage::sign_and_encode(
            &key,
            &Message::file_offer(id, "video.mp4".into(), 1234).unwrap(),
        )
        .unwrap();
        let ticket = iroh_blobs::ticket::BlobTicket::new(
            iroh::EndpointAddr::new(key.public()),
            iroh_blobs::Hash::new(b"video"),
            iroh_blobs::BlobFormat::Raw,
        )
        .to_string();
        let ready = SignedMessage::sign_and_encode(
            &key,
            &Message::FileOfferReady {
                offer_id: id,
                ticket: ticket.clone(),
                thumbnail_hash: None,
            },
        )
        .unwrap();
        let poster = SignedMessage::sign_and_encode(
            &key,
            &Message::FileOfferReady {
                offer_id: id,
                ticket,
                thumbnail_hash: Some([3; 32]),
            },
        )
        .unwrap();
        {
            let store = MessageStore::open(&path).unwrap();
            store
                .persist_direct_offer(&topic, &poster, &[1; 32], None)
                .unwrap();
            store
                .persist_direct_offer(&topic, &announcement, &[1; 32], None)
                .unwrap();
            store
                .persist_direct_offer(&topic, &announcement, &[1; 32], None)
                .unwrap();
            store
                .persist_direct_offer(&topic, &ready, &[1; 32], None)
                .unwrap();
            store
                .set_direct_offer_local_path(&topic, &owner, id, Path::new("/tmp/video.mp4"))
                .unwrap();
        }
        let store = MessageStore::open(&path).unwrap();
        let rows = store.get_messages_for_topic(&topic, 100, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].body, "video.mp4");
        assert_eq!(rows[0].signed_bytes.as_deref(), Some(announcement.as_ref()));
        let state = store
            .direct_offer_state(&topic, &owner, id)
            .unwrap()
            .unwrap();
        assert_eq!(state.ready.as_deref(), Some(poster.as_ref()));
        assert_eq!(state.local_path.as_deref(), Some("/tmp/video.mp4"));
        assert_eq!(
            store
                .get_messages_for_topic(&topic, 100, 0)
                .unwrap()[0]
                .delivery_state,
            "queued"
        );
        store.mark_direct_offer_sent(&topic, &owner, id).unwrap();
        assert_eq!(
            store
                .get_messages_for_topic(&topic, 100, 0)
                .unwrap()[0]
                .delivery_state,
            "sent"
        );
        assert_eq!(
            store
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT unread_count FROM conversation_meta", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(store
            .direct_offer_state(&[9; 32], &owner, id)
            .unwrap()
            .is_none());
        assert!(store
            .direct_offer_state(&topic, &[2; 32], id)
            .unwrap()
            .is_none());
        assert!(store
            .persist_direct_offer(&topic, &announcement, &[1; 32], Some("/untrusted"))
            .is_err());
        let mut forged = announcement.to_vec();
        forged[10] ^= 1;
        assert!(store.persist_direct_offer(&topic, &forged, &[1;32], None).is_err());
        assert_eq!(store.delete_messages_for_topic(&topic).unwrap(), 1);
        assert!(store.direct_offer_state(&topic, &owner, id).unwrap().is_none());
        // Clearing history removes the dedup projection too, not just the card.
        store.persist_direct_offer(&topic, &announcement, &[1;32], None).unwrap();
        assert_eq!(store.get_messages_for_topic(&topic, 100, 0).unwrap().len(), 1);
    }
}
