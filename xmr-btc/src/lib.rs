#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]
use crate::{alice::State, bitcoin::BroadcastSignedTransaction};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{
    task::{Context, Poll},
    Stream,
};
use rand::{CryptoRng, RngCore};
use std::{convert::TryInto, pin::Pin};
use tokio::{
    stream::StreamExt,
    sync::{
        mpsc,
        mpsc::{Receiver, Sender},
    },
};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod monero;

#[derive(Debug)]
pub struct Node<S, R> {
    transport: Transport<S, R>,
}

pub fn new_alice_and_bob() -> (
    Node<alice::Message, bob::Message>,
    Node<bob::Message, alice::Message>,
) {
    let (a_sender, b_receiver): (Sender<alice::Message>, Receiver<alice::Message>) =
        mpsc::channel(5);
    let (b_sender, a_receiver): (Sender<bob::Message>, Receiver<bob::Message>) = mpsc::channel(5);

    let a_transport = Transport {
        sender: a_sender,
        receiver: a_receiver,
    };

    let b_transport = Transport {
        sender: b_sender,
        receiver: b_receiver,
    };

    let alice_node = Node {
        transport: a_transport,
    };

    let bob_node = Node {
        transport: b_transport,
    };

    (alice_node, bob_node)
}

impl Node<alice::Message, bob::Message> {
    pub async fn run<
        R: RngCore + CryptoRng,
        B: bitcoin::GetRawTransaction + BroadcastSignedTransaction,
        M: monero::Transfer,
    >(
        &mut self,
        state0: alice::State0,
        rng: &mut R,
        bitcoin_wallet: &B,
        monero_wallet: &M,
    ) -> Result<alice::State5> {
        self.transport
            .sender
            .send(state0.next_message(rng).into())
            .await?;

        let bob_message0: bob::Message0 = self.transport.receive_message().await?.try_into()?;
        let state1 = state0.receive(bob_message0)?;

        let bob_message1: bob::Message1 = self.transport.receive_message().await?.try_into()?;
        let state2 = state1.receive(bob_message1);
        let alice_message1: alice::Message1 = state2.next_message();
        self.transport.sender.send(alice_message1.into()).await?;

        let bob_message2: bob::Message2 = self.transport.receive_message().await?.try_into()?;
        let state3 = state2.receive(bob_message2)?;

        tokio::time::delay_for(std::time::Duration::new(5, 0)).await;

        tracing::info!("alice is watching for locked btc");
        let state4 = state3.watch_for_lock_btc(bitcoin_wallet).await?;
        let state4b = state4.lock_xmr(monero_wallet).await?;

        self.transport
            .sender
            .send(state4b.next_message().into())
            .await?;

        // pass in state4b as a parameter somewhere in this call to prevent the user
        // from waiting for a message that wont be sent
        let message3: bob::Message3 = self.transport.receive_message().await?.try_into()?;
        // dbg!(&message3);

        let state5 = state4b.receive(message3);

        state5.redeem_btc(bitcoin_wallet).await.unwrap();

        Ok(state5)
    }
}

impl Node<bob::Message, alice::Message> {
    pub async fn run<
        R: RngCore + CryptoRng,
        B: bitcoin::GetRawTransaction
            + bitcoin::BuildTxLockPsbt
            + bitcoin::SignTxLock
            + bitcoin::BroadcastSignedTransaction,
        M: monero::CheckTransfer + monero::ImportOutput,
    >(
        &mut self,
        state0: bob::State0,
        rng: &mut R,
        bitcoin_wallet: &B,
        monero_wallet: &M,
    ) -> Result<bob::State4> {
        self.transport
            .sender
            .send(state0.next_message(rng).into())
            .await?;
        let message0: alice::Message0 = self.transport.receive_message().await?.try_into()?;
        // dbg!(&message0);
        let state1 = state0.receive(bitcoin_wallet, message0).await?;
        self.transport
            .sender
            .send(state1.next_message().into())
            .await?;

        let message1: alice::Message1 = self.transport.receive_message().await?.try_into()?;
        // dbg!(&message1);
        let state2 = state1.receive(message1)?;
        let message2 = state2.next_message();
        let state2b = state2.lock_btc(bitcoin_wallet).await?;
        tracing::info!("bob has locked btc");
        self.transport.sender.send(message2.into()).await?;

        let message2: alice::Message2 = self.transport.receive_message().await?.try_into()?;
        // dbg!(&message2);

        let state3 = state2b.watch_for_lock_xmr(monero_wallet, message2).await?;
        self.transport
            .sender
            .send(state3.next_message().into())
            .await?;

        tracing::info!("bob is watching for redeem_btc");
        tokio::time::delay_for(std::time::Duration::new(5, 0)).await;
        let state4 = state3.watch_for_redeem_btc(bitcoin_wallet).await?;
        state4.claim_xmr(monero_wallet).await?;

        Ok(state4)
    }
}

#[derive(Debug)]
pub struct Transport<S, R> {
    // Using String instead of `Message` implicitly tests the `use-serde` feature.
    sender: Sender<S>,
    receiver: Receiver<R>,
}

#[async_trait]
pub trait SendReceive<S, R> {
    async fn send_message(&mut self, message: S) -> Result<()>;
    async fn receive_message(&mut self) -> Result<R>;
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

#[async_trait]
impl SendReceive<bob::Message, alice::Message> for Transport<bob::Message, alice::Message> {
    async fn send_message(&mut self, message: bob::Message) -> Result<()> {
        let _ = self
            .sender
            .send(message)
            .await
            .map_err(|_| anyhow!("failed to send message"))?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<alice::Message> {
        let message = self
            .receiver
            .next()
            .await
            .ok_or_else(|| anyhow!("failed to receive message"))?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        alice, bitcoin,
        bitcoin::{Amount, TX_FEE},
        bob, monero, new_alice_and_bob,
    };
    use bitcoin_harness::Bitcoind;
    use futures::future;
    use monero_harness::Monero;
    use rand::rngs::OsRng;
    use testcontainers::clients::Cli;
    use tracing_subscriber::util::SubscriberInitExt;

    const TEN_XMR: u64 = 10_000_000_000_000;

    pub async fn init_bitcoind(tc_client: &Cli) -> Bitcoind<'_> {
        let bitcoind = Bitcoind::new(tc_client, "0.19.1").expect("failed to create bitcoind");
        let _ = bitcoind.init(5).await;

        bitcoind
    }

    #[tokio::test]
    async fn happy_path_async() {
        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();

        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let fund_alice = TEN_XMR;
        let fund_bob = 0;
        monero.init(fund_alice, fund_bob).await.unwrap();

        let alice_monero_wallet = monero::AliceWallet(&monero);
        let bob_monero_wallet = monero::BobWallet(&monero);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let alice_initial_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        let bob_initial_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address.clone(),
            punish_address.clone(),
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let (mut alice, mut bob) = new_alice_and_bob();

        let (alice_state5, bob_state4) = future::try_join(
            alice.run(
                alice_state0,
                &mut OsRng,
                &alice_btc_wallet,
                &alice_monero_wallet,
            ),
            bob.run(bob_state0, &mut OsRng, &bob_btc_wallet, &bob_monero_wallet),
        )
        .await
        .unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let lock_tx_bitcoin_fee = bob_btc_wallet
            .transaction_fee(bob_state4.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(
            alice_final_btc_balance,
            alice_initial_btc_balance + btc_amount - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            bob_initial_btc_balance - btc_amount - lock_tx_bitcoin_fee
        );

        let alice_final_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        bob_monero_wallet
            .0
            .wait_for_bob_wallet_block_height()
            .await
            .unwrap();
        let bob_final_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        assert_eq!(
            alice_final_xmr_balance,
            alice_initial_xmr_balance
                - u64::from(xmr_amount)
                - u64::from(alice_state5.lock_xmr_fee())
        );
        assert_eq!(
            bob_final_xmr_balance,
            bob_initial_xmr_balance + u64::from(xmr_amount)
        );
    }

    #[tokio::test]
    async fn both_refund() {
        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let fund_alice = TEN_XMR;
        let fund_bob = 0;

        monero.init(fund_alice, fund_bob).await.unwrap();
        let alice_monero_wallet = monero::AliceWallet(&monero);
        let bob_monero_wallet = monero::BobWallet(&monero);

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let bob_initial_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let alice_message0 = alice_state0.next_message(&mut OsRng);
        let bob_message0 = bob_state0.next_message(&mut OsRng);

        let alice_state1 = alice_state0.receive(bob_message0).unwrap();
        let bob_state1 = bob_state0
            .receive(&bob_btc_wallet, alice_message0)
            .await
            .unwrap();

        let bob_message1 = bob_state1.next_message();
        let alice_state2 = alice_state1.receive(bob_message1);
        let alice_message1 = alice_state2.next_message();
        let bob_state2 = bob_state1.receive(alice_message1).unwrap();

        let bob_message2 = bob_state2.next_message();
        let alice_state3 = alice_state2.receive(bob_message2).unwrap();

        let bob_state2b = bob_state2.lock_btc(&bob_btc_wallet).await.unwrap();

        let alice_state4 = alice_state3
            .watch_for_lock_btc(&alice_btc_wallet)
            .await
            .unwrap();

        let alice_state4b = alice_state4.lock_xmr(&alice_monero_wallet).await.unwrap();

        bob_state2b.refund_btc(&bob_btc_wallet).await.unwrap();

        alice_state4b
            .refund_xmr(&alice_btc_wallet, &alice_monero_wallet)
            .await
            .unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_btc_wallet
            .transaction_fee(bob_state2b.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(alice_final_btc_balance, alice_initial_btc_balance);
        assert_eq!(
            bob_final_btc_balance,
            // The 2 * TX_FEE corresponds to tx_refund and tx_cancel.
            bob_initial_btc_balance - Amount::from_sat(2 * TX_FEE) - lock_tx_bitcoin_fee
        );

        alice_monero_wallet
            .0
            .wait_for_alice_wallet_block_height()
            .await
            .unwrap();
        let alice_final_xmr_balance = alice_monero_wallet.0.get_balance_alice().await.unwrap();
        let bob_final_xmr_balance = bob_monero_wallet.0.get_balance_bob().await.unwrap();

        // Because we create a new wallet when claiming Monero, we can only assert on
        // this new wallet owning all of `xmr_amount` after refund
        assert_eq!(alice_final_xmr_balance, u64::from(xmr_amount));
        assert_eq!(bob_final_xmr_balance, bob_initial_xmr_balance);
    }

    #[tokio::test]
    async fn alice_punishes() {
        let cli = Cli::default();
        let bitcoind = init_bitcoind(&cli).await;

        // must be bigger than our hardcoded fee of 10_000
        let btc_amount = bitcoin::Amount::from_sat(10_000_000);
        let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

        let alice_btc_wallet = bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap();
        let bob_btc_wallet = bitcoin::make_wallet("bob", &bitcoind, btc_amount)
            .await
            .unwrap();

        let alice_initial_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_initial_btc_balance = bob_btc_wallet.balance().await.unwrap();

        let redeem_address = alice_btc_wallet.new_address().await.unwrap();
        let punish_address = redeem_address.clone();
        let refund_address = bob_btc_wallet.new_address().await.unwrap();

        let refund_timelock = 1;
        let punish_timelock = 1;

        let alice_state0 = alice::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            redeem_address,
            punish_address,
        );
        let bob_state0 = bob::State0::new(
            &mut OsRng,
            btc_amount,
            xmr_amount,
            refund_timelock,
            punish_timelock,
            refund_address.clone(),
        );

        let alice_message0 = alice_state0.next_message(&mut OsRng);
        let bob_message0 = bob_state0.next_message(&mut OsRng);

        let alice_state1 = alice_state0.receive(bob_message0).unwrap();
        let bob_state1 = bob_state0
            .receive(&bob_btc_wallet, alice_message0)
            .await
            .unwrap();

        let bob_message1 = bob_state1.next_message();
        let alice_state2 = alice_state1.receive(bob_message1);
        let alice_message1 = alice_state2.next_message();
        let bob_state2 = bob_state1.receive(alice_message1).unwrap();

        let bob_message2 = bob_state2.next_message();
        let alice_state3 = alice_state2.receive(bob_message2).unwrap();

        let bob_state2b = bob_state2.lock_btc(&bob_btc_wallet).await.unwrap();

        let alice_state4 = alice_state3
            .watch_for_lock_btc(&alice_btc_wallet)
            .await
            .unwrap();

        alice_state4.punish(&alice_btc_wallet).await.unwrap();

        let alice_final_btc_balance = alice_btc_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_btc_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_btc_wallet
            .transaction_fee(bob_state2b.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(
            alice_final_btc_balance,
            alice_initial_btc_balance + btc_amount - Amount::from_sat(2 * TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            bob_initial_btc_balance - btc_amount - lock_tx_bitcoin_fee
        );
    }
}
