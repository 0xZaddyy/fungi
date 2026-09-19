//! Linked-mailbox broadcast over a shared secret.
//!
//! [`LinkedMailbox`] lets peers append and stream messages through a
//! [`MailboxStore`] without knowing who else shares the secret.

mod linked;
mod slot;
mod store;

pub use linked::{AppendOnlyMessageSet, LinkedMailbox, MailboxMessages};
pub use slot::{SlotId, derive_slot_id};
pub use store::{MailboxStore, PutOutcome};
