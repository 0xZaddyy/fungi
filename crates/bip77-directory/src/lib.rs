//! BIP77 Payjoin directory storage for Fungi linked mailboxes.
//!
//! This crate maps linked-mailbox slots onto wire-compatible BIP77 mailbox
//! paths. Network implementations provide [`DirectoryExchange`], whose one
//! operation must own a complete request/response exchange. In particular, an
//! OHTTP implementation creates and consumes its request-specific response
//! context entirely inside [`DirectoryExchange::exchange`].

use std::error::Error;
use std::fmt;
use std::future::Future;

use bech32::{Hrp, NoChecksum};
use fungi_mailbox::{MailboxStore, PutOutcome, SlotId};

mod ohttp;

pub use ohttp::{ENCAPSULATED_MESSAGE_BYTES, OhttpExchange, OhttpExchangeError, Relay};

/// A BIP77 mailbox identifier encoded as 13 uppercase bech32 characters.
///
/// BIP77 mailbox paths carry 64 bits without an HRP, separator, or checksum.
/// Mapping a Fungi slot truncates its domain-separated 256-bit identifier.
/// This provides a BIP77-compatible storage path; it does not claim that the
/// path was derived from a Payjoin HPKE public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortId([u8; 8]);

impl ShortId {
    /// Return the identifier bytes.
    pub fn to_bytes(self) -> [u8; 8] {
        self.0
    }
}

impl From<SlotId> for ShortId {
    fn from(slot: SlotId) -> Self {
        let mut id = [0; 8];
        id.copy_from_slice(&slot.to_bytes()[..8]);
        Self(id)
    }
}

impl fmt::Display for ShortId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hrp = Hrp::parse("ID").map_err(|_| fmt::Error)?;
        let encoded = bech32::encode_upper::<NoChecksum>(hrp, &self.0).map_err(|_| fmt::Error)?;
        formatter.write_str(encoded.strip_prefix("ID1").ok_or(fmt::Error)?)
    }
}

impl fmt::Debug for ShortId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ShortId")
            .field(&self.to_string())
            .finish()
    }
}

/// Inner HTTP method sent to the BIP77 target resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Retrieve or long-poll one mailbox.
    Get,
    /// Store one mailbox payload.
    Post,
}

/// One inner request to a BIP77 directory target resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryRequest {
    /// Inner HTTP method.
    pub method: Method,
    /// Absolute target-resource URL.
    pub target: String,
    /// Inner request body.
    pub body: Vec<u8>,
}

/// One decapsulated response from a BIP77 directory target resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryResponse {
    /// Inner HTTP status code.
    pub status: u16,
    /// Inner response body.
    pub body: Vec<u8>,
}

/// Execute one complete directory request/response transaction.
///
/// Implementations must not expose request-correlated state to callers. An
/// OHTTP implementation must create fresh encapsulation for every call and
/// use that call's response context before returning.
pub trait DirectoryExchange: Send + Sync {
    /// Exchange failure.
    type Error: Error + Send + Sync + 'static;

    /// Execute one complete exchange.
    fn exchange(
        &self,
        request: DirectoryRequest,
    ) -> impl Future<Output = Result<DirectoryResponse, Self::Error>> + Send;
}

/// Failure to use a BIP77 directory as a linked-mailbox store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError<E: Error + 'static> {
    /// The underlying exchange failed.
    #[error("directory exchange failed: {0}")]
    Exchange(#[source] E),
    /// The directory returned a status not defined for the operation.
    #[error("directory returned unexpected status {status} for {method:?}")]
    UnexpectedStatus {
        /// Operation that received the status.
        method: Method,
        /// Unexpected inner HTTP status.
        status: u16,
    },
    /// A successful POST was not observable in the following verification GET.
    #[error("directory accepted a POST but its mailbox remained empty")]
    MissingAfterPost,
}

/// A BIP77 directory exposed through the linked-mailbox storage contract.
#[derive(Debug)]
pub struct Bip77MailboxStore<X> {
    exchange: X,
    directory: String,
}

impl<X> Bip77MailboxStore<X> {
    /// Construct a store for a directory target-resource base URL.
    pub fn new(exchange: X, directory: impl Into<String>) -> Self {
        Self {
            exchange,
            directory: directory.into().trim_end_matches('/').to_owned(),
        }
    }

    fn target(&self, slot: SlotId) -> String {
        format!("{}/{}", self.directory, ShortId::from(slot))
    }
}

impl<X: DirectoryExchange> Bip77MailboxStore<X> {
    async fn request(
        &self,
        method: Method,
        slot: SlotId,
        body: Vec<u8>,
    ) -> Result<DirectoryResponse, StoreError<X::Error>> {
        self.exchange
            .exchange(DirectoryRequest {
                method,
                target: self.target(slot),
                body,
            })
            .await
            .map_err(StoreError::Exchange)
    }

    async fn get_slot(&self, slot: SlotId) -> Result<Option<Vec<u8>>, StoreError<X::Error>> {
        let response = self.request(Method::Get, slot, Vec::new()).await?;
        match response.status {
            200 => Ok(Some(response.body)),
            202 => Ok(None),
            status => Err(StoreError::UnexpectedStatus {
                method: Method::Get,
                status,
            }),
        }
    }
}

impl<X: DirectoryExchange> MailboxStore for Bip77MailboxStore<X> {
    type Message = Vec<u8>;
    type Error = StoreError<X::Error>;

    async fn put(&self, slot: SlotId, message: &Self::Message) -> Result<PutOutcome, Self::Error> {
        let response = self.request(Method::Post, slot, message.clone()).await?;
        if response.status != 200 {
            return Err(StoreError::UnexpectedStatus {
                method: Method::Post,
                status: response.status,
            });
        }

        // The reference directory deliberately returns 200 for both a new
        // write and an occupied mailbox. Its atomic first-writer-wins storage
        // lets a non-destructive GET identify which payload won the race.
        match self.get_slot(slot).await? {
            Some(stored) if stored == *message => Ok(PutOutcome::Stored),
            Some(_) => Ok(PutOutcome::Occupied),
            None => Err(StoreError::MissingAfterPost),
        }
    }

    async fn get(&self, slot: SlotId) -> Result<Option<Self::Message>, Self::Error> {
        self.get_slot(slot).await
    }
}

#[cfg(test)]
mod tests;
