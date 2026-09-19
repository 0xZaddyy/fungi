//! PR #32 transport capabilities backed by a BIP77 linked mailbox.

use std::sync::Arc;

use fungi_mailbox::{AppendOnlyMessageSet, LinkedMailbox, SlotId, derive_slot_id};
use fungi_transport::{Anonymous, Channel, PeerChannel, RecvChannel, SendChannel};
use futures_util::StreamExt;

use crate::{Bip77MailboxStore, OhttpExchange, OhttpExchangeError, Relay, StoreError};

type OhttpMailbox<R> = LinkedMailbox<Bip77MailboxStore<OhttpExchange<R>>>;
type ChannelError<R> = StoreError<OhttpExchangeError<<R as Relay>::Error>>;

/// Sending capability for one OHTTP-protected BIP77 linked mailbox.
#[derive(Debug)]
pub struct Bip77Sender<R> {
    mailbox: Arc<OhttpMailbox<R>>,
    peer: SlotId,
}

/// Receiving capability for one OHTTP-protected BIP77 linked mailbox.
#[derive(Debug)]
pub struct Bip77Receiver<R> {
    mailbox: Arc<OhttpMailbox<R>>,
    peer: SlotId,
    next_index: u64,
}

impl<R> PeerChannel for Bip77Sender<R> {
    type Peer = SlotId;

    fn peer(&self) -> &Self::Peer {
        &self.peer
    }
}

impl<R> PeerChannel for Bip77Receiver<R> {
    type Peer = SlotId;

    fn peer(&self) -> &Self::Peer {
        &self.peer
    }
}

impl<R: Relay> SendChannel<Vec<u8>> for Bip77Sender<R> {
    type Privacy = Anonymous;
    type SendError = ChannelError<R>;

    async fn send(&mut self, message: Vec<u8>) -> Result<(), Self::SendError> {
        self.mailbox.append(message).await
    }
}

impl<R: Relay> RecvChannel<Vec<u8>> for Bip77Receiver<R> {
    type RecvError = ChannelError<R>;

    async fn recv(&mut self) -> Result<Vec<u8>, Self::RecvError> {
        loop {
            let mut messages = self.mailbox.messages_from(self.next_index);
            match messages.next().await {
                Some(Ok(message)) => {
                    self.next_index += 1;
                    return Ok(message);
                }
                Some(Err(error)) => return Err(error),
                None => {}
            }
            // A BIP77 202 ends one long-poll window, not the channel. Keeping
            // the index unchanged also makes cancellation safe.
        }
    }
}

/// Build independently owned transport directions over a BIP77 OHTTP mailbox.
///
/// Each `send` and each individual mailbox poll owns its complete OHTTP
/// request/response context. Consequently splitting the returned channel never
/// splits transactional OHTTP state. The relay must permit concurrent calls so
/// a pending receive does not prevent sending progress.
///
/// `Anonymous` describes sender privacy relative to the directory under the
/// OHTTP non-collusion model. It does not promise unlinkability between mailbox
/// operations or anonymity if the relay and directory collude.
pub fn ohttp_mailbox_channel<R: Relay>(
    exchange: OhttpExchange<R>,
    directory: impl Into<String>,
    shared_secret: [u8; 32],
) -> Channel<Bip77Sender<R>, Bip77Receiver<R>> {
    let mailbox = Arc::new(LinkedMailbox::new(
        Bip77MailboxStore::new(exchange, directory),
        shared_secret,
    ));
    let peer = derive_slot_id(&shared_secret, 0);

    Channel::new(
        Bip77Sender {
            mailbox: Arc::clone(&mailbox),
            peer,
        },
        Bip77Receiver {
            mailbox,
            peer,
            next_index: 0,
        },
    )
    .expect("both directions are constructed for the same mailbox")
}
