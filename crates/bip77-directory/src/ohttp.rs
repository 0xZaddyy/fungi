//! Complete BIP77 OHTTP request/response exchanges.

use std::error::Error;
use std::future::Future;
use std::io::Cursor;

use rand::RngCore;
use url::Url;

use crate::{DirectoryExchange, DirectoryRequest, DirectoryResponse, Method};

/// Fixed BIP77 encapsulated request and response size.
pub const ENCAPSULATED_MESSAGE_BYTES: usize = 8192;

const PADDED_BHTTP_REQUEST_BYTES: usize = 8104;

/// Submit one encapsulated request to an OHTTP relay.
///
/// Implementations must perform one HTTP submission without automatically
/// replaying `body`. Retrying the same OHTTP ciphertext makes requests
/// trivially linkable. Callers retry by invoking [`DirectoryExchange::exchange`]
/// again, which creates fresh padding and a fresh OHTTP context.
pub trait Relay: Send + Sync {
    /// Relay transport failure.
    type Error: Error + Send + Sync + 'static;

    /// Submit one encapsulated message and return its encapsulated response.
    fn post(&self, body: Vec<u8>) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

/// Failure during a complete BIP77 OHTTP exchange.
#[derive(Debug, thiserror::Error)]
pub enum OhttpExchangeError<E: Error + 'static> {
    /// The target-resource URL is invalid or unsupported.
    #[error("invalid directory target: {0}")]
    Target(#[source] url::ParseError),
    /// Binary HTTP encoding or decoding failed.
    #[error("binary HTTP: {0}")]
    Bhttp(#[source] bhttp::Error),
    /// OHTTP configuration, encapsulation, or decapsulation failed.
    #[error("OHTTP: {0}")]
    Ohttp(#[source] ohttp::Error),
    /// The relay transport failed.
    #[error("relay: {0}")]
    Relay(#[source] E),
    /// Encapsulation did not produce the fixed BIP77 message size.
    #[error("encapsulated request has size {actual}, expected {expected}")]
    RequestSize {
        /// Actual encoded size.
        actual: usize,
        /// Required BIP77 size.
        expected: usize,
    },
    /// The relay returned a response of the wrong size.
    #[error("encapsulated response has size {actual}, expected {expected}")]
    ResponseSize {
        /// Actual encoded size.
        actual: usize,
        /// Required BIP77 size.
        expected: usize,
    },
    /// A decoded BHTTP response did not contain a final status.
    #[error("binary HTTP response has no final status")]
    MissingStatus,
}

/// Complete OHTTP directory exchanges over an injected relay transport.
#[derive(Debug)]
pub struct OhttpExchange<R> {
    relay: R,
    key_config: Vec<u8>,
}

impl<R> OhttpExchange<R> {
    /// Validate and retain an encoded RFC 9458 OHTTP key configuration.
    pub fn new(relay: R, key_config: impl Into<Vec<u8>>) -> Result<Self, ohttp::Error> {
        let key_config = key_config.into();
        ohttp::KeyConfig::decode(&key_config)?;
        Ok(Self { relay, key_config })
    }

    fn encode_request<E: Error + 'static>(
        &self,
        request: DirectoryRequest,
    ) -> Result<(Vec<u8>, ohttp::ClientResponse), OhttpExchangeError<E>> {
        let url = Url::parse(&request.target).map_err(OhttpExchangeError::Target)?;
        let authority = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_owned(),
        };
        let mut path = url.path().to_owned();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }

        let method = match request.method {
            Method::Get => b"GET".to_vec(),
            Method::Post => b"POST".to_vec(),
        };
        let mut message = bhttp::Message::request(
            method,
            url.scheme().as_bytes().to_vec(),
            authority.into_bytes(),
            path.into_bytes(),
        );
        if !request.body.is_empty() {
            message.write_content(&request.body);
        }

        let mut padded = [0; PADDED_BHTTP_REQUEST_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut padded);
        message
            .write_bhttp(bhttp::Mode::KnownLength, &mut padded.as_mut_slice())
            .map_err(OhttpExchangeError::Bhttp)?;

        let client = ohttp::ClientRequest::from_encoded_config(&self.key_config)
            .map_err(OhttpExchangeError::Ohttp)?;
        let (encapsulated, response_context) = client
            .encapsulate(&padded)
            .map_err(OhttpExchangeError::Ohttp)?;
        if encapsulated.len() != ENCAPSULATED_MESSAGE_BYTES {
            return Err(OhttpExchangeError::RequestSize {
                actual: encapsulated.len(),
                expected: ENCAPSULATED_MESSAGE_BYTES,
            });
        }
        Ok((encapsulated, response_context))
    }

    fn decode_response<E: Error + 'static>(
        response: Vec<u8>,
        context: ohttp::ClientResponse,
    ) -> Result<DirectoryResponse, OhttpExchangeError<E>> {
        if response.len() != ENCAPSULATED_MESSAGE_BYTES {
            return Err(OhttpExchangeError::ResponseSize {
                actual: response.len(),
                expected: ENCAPSULATED_MESSAGE_BYTES,
            });
        }
        let plaintext = context
            .decapsulate(&response)
            .map_err(OhttpExchangeError::Ohttp)?;
        let message = bhttp::Message::read_bhttp(&mut Cursor::new(plaintext))
            .map_err(OhttpExchangeError::Bhttp)?;
        let status = message
            .control()
            .status()
            .ok_or(OhttpExchangeError::MissingStatus)?
            .code();
        Ok(DirectoryResponse {
            status,
            body: message.content().to_vec(),
        })
    }
}

impl<R: Relay> DirectoryExchange for OhttpExchange<R> {
    type Error = OhttpExchangeError<R::Error>;

    async fn exchange(&self, request: DirectoryRequest) -> Result<DirectoryResponse, Self::Error> {
        let (request, context) = self.encode_request(request)?;
        let response = self
            .relay
            .post(request)
            .await
            .map_err(OhttpExchangeError::Relay)?;
        Self::decode_response(response, context)
    }
}
