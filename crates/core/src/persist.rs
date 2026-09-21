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
