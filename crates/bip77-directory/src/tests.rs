use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Mutex;

use fungi_mailbox::{MailboxStore, PutOutcome, derive_slot_id};

use super::*;

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
