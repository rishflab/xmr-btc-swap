use crate::{alice, bitcoin, bob, monero, SendReceive, Transport};
use anyhow::Result;
use genawaiter::sync::Gen;
use std::{convert::TryInto, future::Future};

use genawaiter::GeneratorState;
use rand::{CryptoRng, RngCore};

use crate::bitcoin::BroadcastSignedTransaction;

// todo: move params fron run_alice function into this struct
pub struct Alice;

pub async fn run_alice_until(
    mut gen: Gen<alice::State, (), impl Future<Output = anyhow::Result<alice::State>>>,
    is_state: fn(&alice::State) -> bool,
) -> Result<alice::State> {
    loop {
        match gen.async_resume().await {
            GeneratorState::Yielded(y) => {
                if is_state(&y) {
                    return Ok(y);
                }
            }
            GeneratorState::Complete(r) => return r,
        }
    }
}

impl Alice {
    pub fn run_alice<
        'a,
        R: RngCore + CryptoRng,
        B: bitcoin::GetRawTransaction + BroadcastSignedTransaction,
        M: monero::Transfer,
    >(
        &'a mut self,
        transport: &'a mut Transport<alice::Message, bob::Message>,
        state0: alice::State0,
        rng: &'a mut R,
        bitcoin_wallet: &'a B,
        monero_wallet: &'a M,
    ) -> Gen<alice::State, (), impl Future<Output = anyhow::Result<alice::State>> + 'a> {
        Gen::new(|co| async move {
            transport
                .sender
                .send(state0.next_message(rng).into())
                .await?;

            let bob_message0: bob::Message0 = transport.receive_message().await?.try_into()?;
            let state1 = state0.receive(bob_message0)?;
            co.yield_(alice::State::State1(state1.clone())).await;

            let bob_message1: bob::Message1 = transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(bob_message1);
            let alice_message1: alice::Message1 = state2.next_message();
            transport.sender.send(alice_message1.into()).await?;
            co.yield_(alice::State::State2(state2.clone())).await;

            let bob_message2: bob::Message2 = transport.receive_message().await?.try_into()?;
            let state3 = state2.receive(bob_message2)?;
            co.yield_(alice::State::State3(state3.clone())).await;

            tokio::time::delay_for(std::time::Duration::new(5, 0)).await;

            tracing::info!("alice is watching for locked btc");
            let state4 = state3.watch_for_lock_btc(bitcoin_wallet).await?;
            co.yield_(alice::State::State4(state4.clone())).await;

            let state4b = state4.lock_xmr(monero_wallet).await?;
            tracing::info!("alice has locked xmr");
            co.yield_(alice::State::State4b(state4b.clone())).await;

            transport.sender.send(state4b.next_message().into()).await?;

            // pass in state4b as a parameter somewhere in this call to prevent the user
            // from waiting for a message that wont be sent
            let message3: bob::Message3 = transport.receive_message().await?.try_into()?;

            let state5 = state4b.receive(message3);
            state5.redeem_btc(bitcoin_wallet).await.unwrap();
            co.yield_(alice::State::State5(state5.clone())).await;

            Ok(alice::State::from(state5))
        })
    }
}
