use crate::client::RkvmWriter;

use rkvm_input::{event::Event, writer::EventWriter};
use rkvm_net::Update;

use async_trait::async_trait;
use std::io::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

const EVENT_LOG_LIMIT: usize = 20;
static FORWARDED_EVENTS: AtomicUsize = AtomicUsize::new(0);

pub struct ClientWriter<W> {
    writer: W,
}

impl<W> ClientWriter<W>
where W: RkvmWriter + Send {
    pub fn new(writer: W) -> Self {
        ClientWriter { writer: writer }
    }
}

#[async_trait]
impl<W> EventWriter for ClientWriter<W>
where W: RkvmWriter + Send {
    async fn event(&mut self, id: usize, event: Event) -> Result<(), Error> {
            let event_number = FORWARDED_EVENTS.fetch_add(1, Ordering::Relaxed);
            if event_number < EVENT_LOG_LIMIT {
                tracing::info!(
                    event_number = event_number + 1,
                    device_id = id,
                    event = ?event,
                    "Forwarding input event to injector"
                );
            }
            if let Err(e) = self.writer.send(Update::Event {id, event}).await {
                tracing::warn!("Failed to send update to client {:?}", e);
            }
            Ok(())
    }
}
