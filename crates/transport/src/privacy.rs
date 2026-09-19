//! Types for declaring sender privacy guarantees relative to the destination peer.

mod sealed {
    pub trait Sealed {}
}

/// A declared transport privacy guarantee.
///
/// Code that requires message unlinkability should require a channel
/// tagged with [`MessageUnlinkability`].
pub trait Privacy: sealed::Sealed + Send + Sync {}

/// Messages cannot be linked to a sender identifier or to one another.
#[derive(Debug)]
pub enum MessageUnlinkability {}

/// Messages may be linked to one another, but not to a sender identifier.
#[derive(Debug)]
pub enum ConnectionUnlinkability {}

/// Sender anonymity remains unspecified.
///
/// This includes authenticated transports and configurations awaiting verification
/// of their privacy properties.
#[derive(Debug)]
pub enum Unspecified {}

impl sealed::Sealed for MessageUnlinkability {}
impl sealed::Sealed for ConnectionUnlinkability {}
impl sealed::Sealed for Unspecified {}
impl Privacy for MessageUnlinkability {}
impl Privacy for ConnectionUnlinkability {}
impl Privacy for Unspecified {}
