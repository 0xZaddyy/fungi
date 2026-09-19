//! Message-oriented transport traits for the Fungi protocol.

mod privacy;
mod traits;

pub use privacy::{ConnectionUnlinkability, MessageUnlinkability, Privacy, Unspecified};
pub use traits::{Channel, ChannelBuilder, RecvChannel, SendChannel};
