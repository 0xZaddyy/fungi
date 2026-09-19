//! Storage contract for one slot in a linked mailbox chain.

use std::error::Error;
use std::future::Future;

use crate::slot::SlotId;

/// Result of attempting to store a message in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    /// The slot was empty; the message is now stored there.
    Stored,
    /// A different message already occupies this slot.
    Occupied,
}

/// Storage for one linked-mailbox chain, keyed by [`SlotId`].
///
/// [`put`](MailboxStore::put) must atomically claim a slot. Concurrent calls
/// for the same slot cannot both return [`PutOutcome::Stored`].
pub trait MailboxStore: Send + Sync {
    /// Message stored in one slot.
    type Message: Send + Sync;
    /// Failure to read or write a slot.
    type Error: Error + Send + Sync + 'static;

    /// Store `message` at `slot_id` unless the slot is already occupied.
    fn put(
        &self,
        slot_id: SlotId,
        message: &Self::Message,
    ) -> impl Future<Output = Result<PutOutcome, Self::Error>> + Send;

    /// Fetch the message stored at `slot_id`, waiting out an
    /// implementation-defined window before reporting it empty.
    ///
    /// [`messages`](crate::AppendOnlyMessageSet::messages) stops at the first
    /// `None` this returns, so that wait is what turns "nothing here yet"
    /// into "no writer showed up in a fair window" — a considered guess, not
    /// a guarantee, since a slower writer can still claim this slot after
    /// the wait elapses.
    fn get(
        &self,
        slot_id: SlotId,
    ) -> impl Future<Output = Result<Option<Self::Message>, Self::Error>> + Send;
}

impl<T: MailboxStore> MailboxStore for &T {
    type Message = T::Message;
    type Error = T::Error;

    async fn put(
        &self,
        slot_id: SlotId,
        message: &Self::Message,
    ) -> Result<PutOutcome, Self::Error> {
        (**self).put(slot_id, message).await
    }

    async fn get(&self, slot_id: SlotId) -> Result<Option<Self::Message>, Self::Error> {
        (**self).get(slot_id).await
    }
}
