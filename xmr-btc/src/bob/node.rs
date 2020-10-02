use crate::{
    alice, bitcoin,
    bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock},
    bob, monero,
    transport::SendReceive,
};
use anyhow::Result;

use crate::transport::Transport;
use rand::{CryptoRng, RngCore};
use std::convert::TryInto;

// This struct is responsible for I/O
pub struct Node<'a> {
    transport: Transport<bob::Message, alice::Message>,
    pub bitcoin_wallet: crate::bitcoin::Wallet,
    pub monero_wallet: crate::monero::BobWallet<'a>,
}

impl<'a> Node<'a> {
    pub fn new(
        transport: Transport<bob::Message, alice::Message>,
        bitcoin_wallet: crate::bitcoin::Wallet,
        monero_wallet: crate::monero::BobWallet<'a>,
    ) -> Node<'a> {
        Self {
            transport,
            bitcoin_wallet,
            monero_wallet,
        }
    }
}

pub async fn run_bob_until<'a, R: RngCore + CryptoRng>(
    bob: &mut Node<'a>,
    initial_state: bob::State,
    is_state: fn(&bob::State) -> bool,
    rng: &mut R,
) -> Result<bob::State> {
    let mut result = initial_state;
    loop {
        result = next_state(bob, result, rng).await?;
        if is_state(&result) {
            return Ok(result);
        }
    }
}

async fn next_state<'a, R: RngCore + CryptoRng>(
    node: &mut Node<'a>,
    state: bob::State,
    rng: &mut R,
) -> Result<bob::State> {
    match state {
        bob::State::State0(state0) => {
            node.transport
                .sender
                .send(state0.next_message(rng).into())
                .await?;
            let message0: alice::Message0 = node.transport.receive_message().await?.try_into()?;
            let state1 = state0.receive(&node.bitcoin_wallet, message0).await?;
            Ok(state1.into())
        }
        bob::State::State1(state1) => {
            node.transport
                .sender
                .send(state1.next_message().into())
                .await?;

            let message1: alice::Message1 = node.transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(message1)?;
            Ok(state2.into())
        }
        bob::State::State2(state2) => {
            let message2 = state2.next_message();
            let state2b = state2.lock_btc(&node.bitcoin_wallet).await?;
            tracing::info!("bob has locked btc");
            &node.transport.sender.send(message2.into()).await?;
            Ok(state2b.into())
        }
        bob::State::State2b(state2b) => {
            let message2: alice::Message2 = node.transport.receive_message().await?.try_into()?;

            let state3 = state2b
                .watch_for_lock_xmr(&node.monero_wallet, message2)
                .await?;
            tracing::info!("bob has seen that alice has locked xmr");
            Ok(state3.into())
        }
        bob::State::State3(state3) => {
            node.transport
                .sender
                .send(state3.next_message().into())
                .await?;

            tracing::info!("bob is watching for redeem_btc");
            tokio::time::delay_for(std::time::Duration::new(5, 0)).await;
            let state4 = state3.watch_for_redeem_btc(&node.bitcoin_wallet).await?;
            tracing::info!("bob has seen that alice has redeemed btc");
            state4.claim_xmr(&node.monero_wallet).await?;
            tracing::info!("bob has claimed xmr");
            Ok(state4.into())
        }
        bob::State::State4(state4) => Ok(state4.into()),
    }
}
