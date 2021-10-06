//! Types and traits for use in emitting change notifications
pub mod defaultsink;
pub mod filesink;
use super::events::defaultsink::DefaultEventSink;
use super::events::filesink::FileEventSink;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum EventType<T> {
    InvoiceCreated(T),
    MissingParcel(T),
    InvoiceYanked(T),
    ParcelCreated(T),
}

#[async_trait::async_trait]
pub trait EventSink {
    // Emits an event.
    async fn raise_event<T>(&self, _: EventType<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: Serialize + Send + Sync;
}

// This enum exists to enable the type of event sync to be dynamically selected at runtime depending on arguments passed
// to the program.
// It seems like the normal approach of using trait objects does not work as the trait is not object safe as it has a generic type parameter.
// parameter, this feels like a bit of a hack, but it's the only way I can see to do this.

pub enum EventSyncs {
    FileEventSink(FileEventSink),
    DefaultEventSink(DefaultEventSink),
}

impl Clone for EventSyncs {
    fn clone(&self) -> Self {
        match self {
            EventSyncs::FileEventSink(sink) => EventSyncs::FileEventSink(sink.clone()),
            EventSyncs::DefaultEventSink(sink) => EventSyncs::DefaultEventSink(sink.clone()),
        }
    }
}

#[async_trait::async_trait]
impl EventSink for EventSyncs {
    async fn raise_event<T>(&self, event: EventType<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: Serialize + Send + Sync,
    {
        match self {
            EventSyncs::FileEventSink(sink) => sink.raise_event(event).await,
            EventSyncs::DefaultEventSink(sink) => sink.raise_event(event).await,
        }
    }
}
