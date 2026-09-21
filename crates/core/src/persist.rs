//! Storage for the event logs a state machine replays.

/// An append only log of the events one session produced.
pub trait Persister {
    /// Whatever the storage layer underneath fails with.
    type InternalStorageError: std::error::Error + Send + Sync + 'static;

    /// The events this session records.
    type SessionEvent;

    /// Append one event to the log.
    fn save_event(&self, event: Self::SessionEvent) -> Result<(), Self::InternalStorageError>;

    /// Every event of the session, in the order they were saved.
    fn load(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Self::SessionEvent>>, Self::InternalStorageError>;

    /// Close the session, after which nothing more is appended.
    fn close(&self) -> Result<(), Self::InternalStorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::convert::Infallible;

    #[derive(Default)]
    struct InMemoryPersister {
        events: RefCell<Vec<u8>>,
        closed: RefCell<bool>,
    }

    impl Persister for InMemoryPersister {
        type InternalStorageError = Infallible;
        type SessionEvent = u8;

        fn save_event(&self, event: u8) -> Result<(), Infallible> {
            self.events.borrow_mut().push(event);
            Ok(())
        }

        fn load(&self) -> Result<Box<dyn Iterator<Item = u8>>, Infallible> {
            Ok(Box::new(self.events.borrow().clone().into_iter()))
        }

        fn close(&self) -> Result<(), Infallible> {
            *self.closed.borrow_mut() = true;
            Ok(())
        }
    }

    #[test]
    fn events_load_in_the_order_they_were_saved() {
        let persister = InMemoryPersister::default();

        persister
            .save_event(1)
            .expect("in memory storage cannot fail");
        persister
            .save_event(2)
            .expect("in memory storage cannot fail");

        let replayed: Vec<u8> = persister
            .load()
            .expect("in memory storage cannot fail")
            .collect();

        assert_eq!(replayed, [1, 2]);
    }

    #[test]
    fn a_closed_session_says_so() {
        let persister = InMemoryPersister::default();

        persister.close().expect("in memory storage cannot fail");

        assert!(*persister.closed.borrow());
    }
}
