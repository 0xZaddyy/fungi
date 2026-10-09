//! BIP77 directory mailbox interface.
//!
//! A [`Request`] gives the HTTP method, mailbox path, and body for a directory
//! request, and interprets the directory's response.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod short_id;

pub use short_id::ShortId;

/// One request to a directory mailbox.
pub trait Request {
    /// Result produced from a successful response.
    type Output;

    /// HTTP method.
    const METHOD: &'static str;

    /// Mailbox identifier.
    fn id(&self) -> ShortId;

    /// Request body.
    fn body(&self) -> &[u8];

    /// Interpret the directory response status and body.
    fn response(status: u16, body: Vec<u8>) -> Result<Self::Output, UnexpectedStatus>;

    /// Mailbox path, to append to the directory URL's existing path.
    ///
    /// Remove a trailing slash from the directory path before appending it.
    fn path(&self) -> String {
        format!("/{}", self.id())
    }
}

/// Retrieve the payload of a mailbox.
///
/// The directory holds the request open until the mailbox has a payload or
/// it times out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Get(pub ShortId);

/// Store a payload in a mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: ShortId,
    pub payload: Vec<u8>,
}

/// The directory answered with a status the request does not define.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("directory returned unexpected status {0}")]
pub struct UnexpectedStatus(pub u16);

impl Request for Get {
    /// The payload, or `None` if the directory timed out before the mailbox
    /// had one.
    type Output = Option<Vec<u8>>;

    const METHOD: &'static str = "GET";

    fn id(&self) -> ShortId {
        self.0
    }

    fn body(&self) -> &[u8] {
        &[]
    }

    fn response(status: u16, body: Vec<u8>) -> Result<Self::Output, UnexpectedStatus> {
        match status {
            200 => Ok(Some(body)),
            202 => Ok(None),
            status => Err(UnexpectedStatus(status)),
        }
    }
}

impl Request for Post {
    type Output = ();

    const METHOD: &'static str = "POST";

    fn id(&self) -> ShortId {
        self.id
    }

    fn body(&self) -> &[u8] {
        &self.payload
    }

    fn response(status: u16, _body: Vec<u8>) -> Result<Self::Output, UnexpectedStatus> {
        match status {
            200 => Ok(()),
            status => Err(UnexpectedStatus(status)),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests;
