use async_std::fs::OpenOptions;
use async_std::io::prelude::*;
use serde::Serialize;
use std::path::PathBuf;
use tracing::info;

use chrono::prelude::*;

use crate::events::{EventSink, EventType};

pub const EVENT_SINK_FILE_NAME: &str = "bindle_event.log";
pub struct FileEventSink {
    file: PathBuf,
}

impl FileEventSink {
    pub fn new(directory: PathBuf) -> Self {
        info!("Using FileEventSink");
        Self {
            file: directory.join(EVENT_SINK_FILE_NAME),
        }
    }
}

impl Clone for FileEventSink {
    fn clone(&self) -> Self {
        FileEventSink {
            file: self.file.clone(),
        }
    }
}

#[derive(Serialize)]
struct EventLog<T> {
    event_date: DateTime<Utc>,
    event_data: EventType<T>,
}

#[async_trait::async_trait]
impl EventSink for FileEventSink {
    async fn raise_event<T>(&self, event: EventType<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: Serialize + Send + Sync,
    {
        let data = EventLog {
            event_date: Utc::now(),
            event_data: event,
        };

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .await?;
        let mut buf = serde_json::to_vec_pretty(&data)?;
        buf.push(b'\n');
        file.write_all(&buf).await?;
        file.flush().await?;
        Ok(())
    }
}

#[cfg(test)]

mod test {

    use super::*;
    use async_std::fs::File;
    use tempfile::tempdir;

    use crate::testing::{BindleEvent, InvoiceYankedEventData};

    #[tokio::test]
    async fn test_should_create_and_write_to_file() {
        let temp = tempdir().expect("unable to create tempdir");
        let directory = PathBuf::from(temp.path());
        let filename = directory.join(EVENT_SINK_FILE_NAME);
        let sink = FileEventSink::new(directory);
        sink.raise_event(EventType::InvoiceYanked("test"))
            .await
            .unwrap();
        let mut file = File::open(filename).await.unwrap();
        let mut buf = String::new();
        file.read_to_string(&mut buf).await.unwrap();
        let event: BindleEvent<InvoiceYankedEventData> = serde_json::from_str(&buf).unwrap();
        assert_eq!("test", event.event_data.invoice_yanked);
    }

    #[tokio::test]
    async fn test_should_create_and_append_to_a_file() {
        let temp = tempdir().expect("unable to create tempdir");
        let directory = PathBuf::from(temp.path());
        let filename = directory.join(EVENT_SINK_FILE_NAME);
        let sink = FileEventSink::new(directory);
        sink.raise_event(EventType::InvoiceYanked("test0"))
            .await
            .unwrap();
        sink.raise_event(EventType::InvoiceYanked("test1"))
            .await
            .unwrap();
        let mut file = File::open(filename).await.unwrap();
        let mut buf = String::new();
        file.read_to_string(&mut buf).await.unwrap();
        let deserializer = serde_json::Deserializer::from_str(&buf);
        let iterator = deserializer.into_iter::<serde_json::Value>();
        for (i, event) in iterator.enumerate() {
            let event: BindleEvent<InvoiceYankedEventData> =
                serde_json::from_value(event.unwrap()).unwrap();
            assert_eq!(format!("test{}", i), event.event_data.invoice_yanked);
        }
    }
}
