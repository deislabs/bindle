use crate::events::{EventSink, EventType};
use serde::Serialize;

pub struct DefaultEventSink {}
impl DefaultEventSink {
    pub fn new() -> Self {
        DefaultEventSink {}
    }
}

impl Clone for DefaultEventSink {
    fn clone(&self) -> Self {
        DefaultEventSink {}
    }
}

#[async_trait::async_trait]
impl EventSink for DefaultEventSink {
    async fn raise_event<T>(&self, _: EventType<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: Serialize + Send + Sync,
    {
        Ok(())
    }
}
