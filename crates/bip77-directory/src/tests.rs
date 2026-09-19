use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use fungi_mailbox::{MailboxStore, PutOutcome, derive_slot_id};

use super::*;

type Ciphertexts = Arc<Mutex<Vec<Vec<u8>>>>;

#[derive(Debug, Default)]
struct ReferenceDirectory {
    mailboxes: Mutex<HashMap<String, Vec<u8>>>,
}

impl DirectoryExchange for ReferenceDirectory {
    type Error = Infallible;

    async fn exchange(&self, request: DirectoryRequest) -> Result<DirectoryResponse, Self::Error> {
        let mut mailboxes = self.mailboxes.lock().unwrap();
        Ok(match request.method {
            Method::Post => {
                mailboxes.entry(request.target).or_insert(request.body);
                // payjoin-mailroom does not expose whether insertion won.
                DirectoryResponse {
                    status: 200,
                    body: Vec::new(),
                }
            }
            Method::Get => match mailboxes.get(&request.target) {
                Some(message) => DirectoryResponse {
                    status: 200,
                    body: message.clone(),
                },
                None => DirectoryResponse {
                    status: 202,
                    body: Vec::new(),
                },
            },
        })
    }
}

#[test]
fn short_ids_use_the_bip77_wire_shape() {
    let id = ShortId::from(derive_slot_id(&[1; 32], 0)).to_string();
    assert_eq!(id.len(), 13);
    assert!(
        id.bytes()
            .all(|byte| b"QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L".contains(&byte))
    );

    assert_eq!(ShortId([0; 8]).to_string(), "QQQQQQQQQQQQQ");
}

#[tokio::test]
async fn post_verification_recovers_first_writer_wins_outcome() {
    let store = Bip77MailboxStore::new(ReferenceDirectory::default(), "https://directory.test/");
    let slot = derive_slot_id(&[2; 32], 0);

    assert_eq!(
        store.put(slot, &b"first".to_vec()).await.unwrap(),
        PutOutcome::Stored
    );
    assert_eq!(
        store.put(slot, &b"second".to_vec()).await.unwrap(),
        PutOutcome::Occupied
    );
    assert_eq!(store.get(slot).await.unwrap(), Some(b"first".to_vec()));
}

#[tokio::test]
async fn concurrent_writers_observe_one_winner() {
    let store = Bip77MailboxStore::new(ReferenceDirectory::default(), "https://directory.test");
    let slot = derive_slot_id(&[4; 32], 0);
    let first_message = b"first".to_vec();
    let second_message = b"second".to_vec();

    let (first, second) = tokio::join!(
        store.put(slot, &first_message),
        store.put(slot, &second_message),
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PutOutcome::Stored)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PutOutcome::Occupied)
            .count(),
        1
    );
}

#[tokio::test]
async fn empty_mailbox_maps_accepted_to_none() {
    let store = Bip77MailboxStore::new(ReferenceDirectory::default(), "https://directory.test");
    assert_eq!(store.get(derive_slot_id(&[3; 32], 0)).await.unwrap(), None);
}

#[derive(Debug)]
struct LoopbackRelay {
    server: ::ohttp::Server,
    ciphertexts: Ciphertexts,
}

impl Relay for LoopbackRelay {
    type Error = Infallible;

    async fn post(&self, body: Vec<u8>) -> Result<Vec<u8>, Self::Error> {
        assert_eq!(body.len(), ENCAPSULATED_MESSAGE_BYTES);
        self.ciphertexts.lock().unwrap().push(body.clone());

        let (plaintext, response_context) = self.server.decapsulate(&body).unwrap();
        assert_eq!(plaintext.len(), 8104);
        let request = bhttp::Message::read_bhttp(&mut Cursor::new(plaintext)).unwrap();
        assert_eq!(request.control().method(), Some(&b"GET"[..]));

        let mut response = bhttp::Message::response(bhttp::StatusCode::try_from(200_u16).unwrap());
        response.write_content(b"mailbox payload");
        let mut padded = [0; 8144];
        response
            .write_bhttp(bhttp::Mode::KnownLength, &mut padded.as_mut_slice())
            .unwrap();
        let encapsulated = response_context.encapsulate(&padded).unwrap();
        assert_eq!(encapsulated.len(), ENCAPSULATED_MESSAGE_BYTES);
        Ok(encapsulated)
    }
}

fn ohttp_loopback() -> (OhttpExchange<LoopbackRelay>, Ciphertexts) {
    use ::ohttp::hpke::{Aead, Kdf, Kem};
    use ::ohttp::{KeyConfig, SymmetricSuite};

    let config = KeyConfig::new(
        1,
        Kem::K256Sha256,
        vec![SymmetricSuite::new(Kdf::HkdfSha256, Aead::ChaCha20Poly1305)],
    )
    .unwrap();
    let server = ::ohttp::Server::new(config).unwrap();
    let encoded_config = server.config().encode().unwrap();
    let ciphertexts = Arc::new(Mutex::new(Vec::new()));
    let exchange = OhttpExchange::new(
        LoopbackRelay {
            server,
            ciphertexts: Arc::clone(&ciphertexts),
        },
        encoded_config,
    )
    .unwrap();
    (exchange, ciphertexts)
}

#[tokio::test]
async fn ohttp_exchange_owns_context_and_never_reuses_ciphertext() {
    let (exchange, ciphertexts) = ohttp_loopback();
    let request = DirectoryRequest {
        method: Method::Get,
        target: "https://directory.test/QQQQQQQQQQQQQ".to_owned(),
        body: Vec::new(),
    };

    let first = exchange.exchange(request.clone()).await.unwrap();
    let second = exchange.exchange(request).await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.body, b"mailbox payload");
    assert_eq!(second, first);

    let ciphertexts = ciphertexts.lock().unwrap();
    assert_eq!(ciphertexts.len(), 2);
    assert_ne!(ciphertexts[0], ciphertexts[1]);
}
