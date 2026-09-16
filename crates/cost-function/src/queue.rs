//! The wallet's outstanding obligations.

use std::fmt::Debug;
use std::hash::Hash;

use crate::intent::IntentWithPolicy;

/// All the information the wallet has about what the user wants to do.
pub trait Queue {
    /// Names one intent in this queue.
    ///
    /// An id for an intent for as long as the queue holds it, and is
    /// never reused for another.
    type Id: Copy + Eq + Hash + Debug;

    /// Every intent with its id, in no particular order.
    fn iter(&self) -> impl Iterator<Item = (Self::Id, &IntentWithPolicy)>;
}
