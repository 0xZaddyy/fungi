//! Append-only broadcast channel over a chain of mailbox slots.

use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_core::Stream;
use futures_util::stream;

use crate::slot::derive_slot_id;
use crate::store::{MailboxStore, PutOutcome};

/// Borrowed stream of messages read from a linked mailbox.
pub type MailboxMessages<'a, S> = Pin<
    Box<
        dyn Stream<Item = Result<<S as MailboxStore>::Message, <S as MailboxStore>::Error>>
            + Send
            + 'a,
    >,
>;

/// An append-only message set shared by peers who hold the same secret.
///
/// Peers append without enrollment or coordination. Reading yields every
/// appended message in an order the backend determines; this trait does not
/// say how that order arises.
pub trait AppendOnlyMessageSet {
    /// One appended message.
    type Message: Send;
    /// Failure to append or stream messages.
    type Error: Error + Send + Sync + 'static;
    /// Stream produced by [`messages`](AppendOnlyMessageSet::messages).
    type Messages<'a>: Stream<Item = Result<Self::Message, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// Append one message.
    fn append(
        &self,
        message: Self::Message,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Stream every message appended so far, in the backend's own order.
    fn messages(&self) -> Self::Messages<'_>;
}

/// [`AppendOnlyMessageSet`] backed by a chain of [`MailboxStore`] slots.
///
/// Slot `i` is [`derive_slot_id(shared_secret, i)`](derive_slot_id).
/// Writers skip occupied slots. Reads stop at the first slot
/// [`MailboxStore::get`] reports empty after waiting out its own poll
/// window — a considered guess, not a guarantee, since a slower writer can
/// still claim that slot afterward.
#[derive(Debug)]
pub struct LinkedMailbox<S> {
    store: S,
    shared_secret: [u8; 32],
    next_append_index: AtomicU64,
}

impl<S> LinkedMailbox<S> {
    /// Create a mailbox chain over `store`, keyed by `shared_secret`.
    pub fn new(store: S, shared_secret: [u8; 32]) -> Self {
        Self {
            store,
            shared_secret,
            next_append_index: AtomicU64::new(0),
        }
    }
}

impl<S: MailboxStore> LinkedMailbox<S> {
    /// Stream messages beginning at `index` in the linked slot chain.
    ///
    /// This lets stateful consumers resume without re-reading earlier slots.
    pub fn messages_from(&self, index: u64) -> MailboxMessages<'_, S> {
        Box::pin(stream::try_unfold(index, move |index| async move {
            let slot_id = derive_slot_id(&self.shared_secret, index);
            Ok(self
                .store
                .get(slot_id)
                .await?
                .map(|message| (message, index + 1)))
        }))
    }
}

impl<S: MailboxStore> AppendOnlyMessageSet for LinkedMailbox<S> {
    type Message = S::Message;
    type Error = S::Error;
    type Messages<'a>
        = Pin<Box<dyn Stream<Item = Result<S::Message, S::Error>> + Send + 'a>>
    where
        Self: 'a;

    async fn append(&self, message: Self::Message) -> Result<(), Self::Error> {
        let mut index = self.next_append_index.load(Ordering::Relaxed);
        loop {
            let slot_id = derive_slot_id(&self.shared_secret, index);
            match self.store.put(slot_id, &message).await? {
                PutOutcome::Stored => {
                    self.next_append_index
                        .fetch_max(index + 1, Ordering::Relaxed);
                    return Ok(());
                }
                PutOutcome::Occupied => index += 1,
            }
        }
    }

    fn messages(&self) -> Self::Messages<'_> {
        self.messages_from(0)
    }
}
