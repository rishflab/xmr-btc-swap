use crate::{
    alice, bitcoin,
    bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock},
    bob, monero, SendReceive, Transport,
};
use anyhow::Result;
use genawaiter::{sync::Gen, GeneratorState};
use rand::{CryptoRng, RngCore};
use std::{convert::TryInto, future::Future};

// todo: move params fro, run_bob function into this struct
pub struct Bob;

pub async fn run_bob_until(
    mut gen: Gen<bob::State, (), impl Future<Output = anyhow::Result<bob::State>>>,
    is_state: fn(&bob::State) -> bool,
) -> Result<bob::State> {
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

impl Bob {
    pub fn run_bob<
        'a,
        R: RngCore + CryptoRng,
        B: bitcoin::GetRawTransaction + BroadcastSignedTransaction + BuildTxLockPsbt + SignTxLock,
        M: monero::CheckTransfer + monero::ImportOutput,
    >(
        &'a mut self,
        transport: &'a mut Transport<bob::Message, alice::Message>,
        state0: bob::State0,
        rng: &'a mut R,
        bitcoin_wallet: &'a B,
        monero_wallet: &'a M,
    ) -> Gen<bob::State, (), impl Future<Output = anyhow::Result<bob::State>> + 'a> {
        Gen::new(|co| async move {
            transport
                .sender
                .send(state0.next_message(rng).into())
                .await?;
            let message0: alice::Message0 = transport.receive_message().await?.try_into()?;
            let state1 = state0.receive(bitcoin_wallet, message0).await?;
            co.yield_(bob::State::State1(state1.clone())).await;
            transport.sender.send(state1.next_message().into()).await?;

            let message1: alice::Message1 = transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(message1)?;
            co.yield_(bob::State::State2(state2.clone())).await;

            let message2 = state2.next_message();
            let state2b = state2.lock_btc(bitcoin_wallet).await?;
            tracing::info!("bob has locked btc");
            transport.sender.send(message2.into()).await?;

            co.yield_(bob::State::State2b(state2b.clone())).await;

            let message2: alice::Message2 = transport.receive_message().await?.try_into()?;

            let state3 = state2b.watch_for_lock_xmr(monero_wallet, message2).await?;
            tracing::info!("bob has seen that alice has locked xmr");
            co.yield_(bob::State::State3(state3.clone())).await;
            transport.sender.send(state3.next_message().into()).await?;

            tracing::info!("bob is watching for redeem_btc");
            tokio::time::delay_for(std::time::Duration::new(5, 0)).await;
            let state4 = state3.watch_for_redeem_btc(bitcoin_wallet).await?;
            tracing::info!("bob has seen that alice has redeemed btc");
            state4.claim_xmr(monero_wallet).await?;
            tracing::info!("bob has claimed xmr");
            co.yield_(bob::State::State4(state4.clone())).await;

            Ok(bob::State::State4(state4))
        })
    }
}
