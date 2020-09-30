use crate::{alice, bob, SendReceive, Transport};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{
    task::{Context, Poll},
    Stream,
};
use std::pin::Pin;

#[derive(Debug)]
pub struct AliceNode {
    transport: Transport<alice::Message, bob::Message>,
    state: alice::State,
}

#[async_trait]
impl SendReceive<alice::Message, bob::Message> for Transport<alice::Message, bob::Message> {
    async fn send_message(&mut self, message: alice::Message) -> Result<()> {
        let _ = self
            .sender
            .send(message)
            .await
            .map_err(|_| anyhow!("failed to send message"))?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<bob::Message> {
        let message = self
            .receiver
            .next()
            .await
            .ok_or_else(|| anyhow!("failed to receive message"))?;
        Ok(message)
    }
}

impl Stream for AliceNode {
    type Item = alice::State;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.state {
            alice::State::State0(..) => {
                let a = self.transport.receive();
                Poll::Pending
            }
            _ => Poll::Ready(None),
        }
    }
}
