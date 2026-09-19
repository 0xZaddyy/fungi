//! Mailbox behavior through the public API.

use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::convert::Infallible;
use std::sync::Mutex;
use std::time::Duration;

use fungi_mailbox::{
    AppendOnlyMessageSet, LinkedMailbox, MailboxStore, PutOutcome, SlotId, derive_slot_id,
};
use futures_util::StreamExt;
use tokio::sync::Notify;

/// In-memory [`MailboxStore`] whose `get` long-polls: an unfilled slot is
/// checked, then re-checked whenever any slot is written, until
/// `poll_timeout` elapses. This mirrors a directory server holding a GET
/// open until a payload lands or the wait times out, which is what makes
/// [`AppendOnlyMessageSet::messages`]'s "stop at the first empty slot" rule
/// a considered guess rather than an instant snapshot.
#[derive(Debug)]
struct MemStore {
    slots: Mutex<HashMap<SlotId, Vec<u8>>>,
    notify: Notify,
    poll_timeout: Duration,
}

impl MemStore {
    fn new(poll_timeout: Duration) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            poll_timeout,
        }
    }
}

impl Default for MemStore {
    fn default() -> Self {
        Self::new(Duration::from_millis(50))
    }
}

impl MailboxStore for MemStore {
    type Message = Vec<u8>;
    type Error = Infallible;

    async fn put(&self, slot_id: SlotId, message: &Vec<u8>) -> Result<PutOutcome, Infallible> {
        let outcome = {
            let mut slots = self.slots.lock().unwrap();
            match slots.entry(slot_id) {
                Entry::Occupied(_) => PutOutcome::Occupied,
                Entry::Vacant(entry) => {
                    entry.insert(message.clone());
                    PutOutcome::Stored
                }
            }
        };
        if outcome == PutOutcome::Stored {
            self.notify.notify_waiters();
        }
        Ok(outcome)
    }

    async fn get(&self, slot_id: SlotId) -> Result<Option<Vec<u8>>, Infallible> {
        loop {
            // Registering interest before checking the slot, rather than
            // after, is what makes a `put` that lands between the check and
            // the wait still wake this call instead of being missed for a
            // full `poll_timeout`.
            let notified = self.notify.notified();
            if let Some(message) = self.slots.lock().unwrap().get(&slot_id).cloned() {
                return Ok(Some(message));
            }
            if tokio::time::timeout(self.poll_timeout, notified)
                .await
                .is_err()
            {
                return Ok(None);
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn slot_is_first_writer_wins() {
    let store = MemStore::default();
    let slot_id = derive_slot_id(&[9; 32], 0);

    assert_eq!(
        store.put(slot_id, &b"first".to_vec()).await.unwrap(),
        PutOutcome::Stored
    );
    assert_eq!(
        store.put(slot_id, &b"second".to_vec()).await.unwrap(),
        PutOutcome::Occupied
    );
    assert_eq!(store.get(slot_id).await.unwrap(), Some(b"first".to_vec()));
}

#[tokio::test(start_paused = true)]
async fn empty_slot_is_absent() {
    let store = MemStore::default();
    assert_eq!(store.get(derive_slot_id(&[1; 32], 0)).await.unwrap(), None);
}

#[tokio::test(start_paused = true)]
async fn concurrent_appends_are_persisted() {
    let store = MemStore::default();
    let secret = [42; 32];
    let alice = LinkedMailbox::new(&store, secret);
    let bob = LinkedMailbox::new(&store, secret);
    let carol = LinkedMailbox::new(&store, secret);

    let results = tokio::join!(
        alice.append(b"alice".to_vec()),
        bob.append(b"bob".to_vec()),
        carol.append(b"carol".to_vec()),
    );
    results.0.unwrap();
    results.1.unwrap();
    results.2.unwrap();

    let observed: HashSet<Vec<u8>> = alice
        .messages()
        .map(|message| message.unwrap())
        .collect()
        .await;
    let expected = [b"alice".to_vec(), b"bob".to_vec(), b"carol".to_vec()]
        .into_iter()
        .collect();
    assert_eq!(observed, expected);
}

#[tokio::test(start_paused = true)]
async fn different_secret_sees_no_messages() {
    let store = MemStore::default();
    LinkedMailbox::new(&store, [1; 32])
        .append(b"private".to_vec())
        .await
        .unwrap();

    let stranger = LinkedMailbox::new(&store, [2; 32]);
    let messages = stranger.messages();
    tokio::pin!(messages);
    assert!(messages.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn messages_stop_only_after_the_poll_window_elapses() {
    let store = MemStore::new(Duration::from_millis(50));
    let mailbox = LinkedMailbox::new(&store, [3; 32]);
    mailbox.append(b"only".to_vec()).await.unwrap();

    let messages = mailbox.messages();
    tokio::pin!(messages);
    assert_eq!(messages.next().await.unwrap().unwrap(), b"only");

    let started = tokio::time::Instant::now();
    assert!(messages.next().await.is_none());
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "stopping must wait out the poll window, not just snapshot the slot"
    );
}

#[tokio::test(start_paused = true)]
async fn append_skips_an_occupied_slot() {
    let store = MemStore::default();
    let secret = [4; 32];
    store
        .put(derive_slot_id(&secret, 0), &b"occupied".to_vec())
        .await
        .unwrap();

    LinkedMailbox::new(&store, secret)
        .append(b"mine".to_vec())
        .await
        .unwrap();
    assert_eq!(
        store.get(derive_slot_id(&secret, 1)).await.unwrap(),
        Some(b"mine".to_vec())
    );
}

#[tokio::test(start_paused = true)]
async fn get_observes_a_write_that_lands_within_the_poll_window() {
    let store = MemStore::new(Duration::from_millis(100));
    let slot_id = derive_slot_id(&[5; 32], 0);

    let write_after_a_delay = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        store.put(slot_id, &b"just in time".to_vec()).await.unwrap();
    };
    let read = store.get(slot_id);

    let (_, observed) = futures_util::future::join(write_after_a_delay, read).await;
    assert_eq!(observed.unwrap(), Some(b"just in time".to_vec()));
}

#[tokio::test(start_paused = true)]
async fn a_write_after_the_poll_window_can_be_missed() {
    let store = MemStore::new(Duration::from_millis(50));
    let slot_id = derive_slot_id(&[6; 32], 0);

    let late_write = async {
        tokio::time::sleep(Duration::from_millis(60)).await;
        store.put(slot_id, &b"too late".to_vec()).await.unwrap();
    };
    let read = store.get(slot_id);

    let (_, observed) = futures_util::future::join(late_write, read).await;
    assert_eq!(
        observed.unwrap(),
        None,
        "a write landing after the poll window is not guaranteed to be seen"
    );
}
