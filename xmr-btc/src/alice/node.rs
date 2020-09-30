use crate::{alice, bob, SendReceive, Transport};
use anyhow::{ Result};
use genawaiter::sync::{Gen};
use std::{
    future::Future,
    convert::TryInto,
};
use crate::bitcoin;
use crate::monero;

#[derive(Debug)]
pub struct AliceNode {
    pub(crate) transport: Transport<alice::Message, bob::Message>,
}
use genawaiter::{GeneratorState};
use rand::{CryptoRng, RngCore};

use crate::bitcoin::BroadcastSignedTransaction;


pub async fn run_alice_until(mut gen: Gen<alice::State, (), impl Future<Output = anyhow::Result<alice::State>>>, state: alice::State) -> Result<alice::State> {
    loop {
        match gen.async_resume().await {
            GeneratorState::Yielded(y) => {
                if  std::mem::discriminant(&y) == std::mem::discriminant(&state) {
                    return Ok(y)
                }

            }
            GeneratorState::Complete(r) => {
                return r
            }
        }
    }
}


pub async fn run_alice<
    R: RngCore + CryptoRng,
    B: bitcoin::GetRawTransaction + BroadcastSignedTransaction,
    M: monero::Transfer,
>(
    node: &'static mut AliceNode,
    state0: alice::State0,
    rng: &'static mut R,
    bitcoin_wallet: &'static B,
    monero_wallet: &'static M,
) -> Gen<alice::State, (), impl Future<Output = anyhow::Result<alice::State>>> {
    Gen::new(|co| async move {
        node.transport
            .sender
            .send(state0.next_message(rng).into())
            .await?;

        let bob_message0: bob::Message0 = node.transport.receive_message().await?.try_into()?;
        let state1 = state0.receive(bob_message0)?;
        co.yield_(alice::State::State1(state1.clone())).await;

        let bob_message1: bob::Message1 = node.transport.receive_message().await?.try_into()?;
        let state2 = state1.receive(bob_message1);
        let alice_message1: alice::Message1 = state2.next_message();
        node.transport.sender.send(alice_message1.into()).await?;
        co.yield_(alice::State::State2(state2.clone())).await;

        let bob_message2: bob::Message2 = node.transport.receive_message().await?.try_into()?;
        let state3 = state2.receive(bob_message2)?;
        co.yield_(alice::State::State3(state3.clone())).await;

        tokio::time::delay_for(std::time::Duration::new(5, 0)).await;

        tracing::info!("alice is watching for locked btc");
        let state4 = state3.watch_for_lock_btc(bitcoin_wallet).await?;
        co.yield_(alice::State::State4(state4.clone())).await;

        let state4b = state4.lock_xmr(monero_wallet).await?;
        co.yield_(alice::State::State4b(state4b.clone())).await;

        node.transport
            .sender
            .send(state4b.next_message().into())
            .await?;

        // pass in state4b as a parameter somewhere in this call to prevent the user
        // from waiting for a message that wont be sent
        let message3: bob::Message3 = node.transport.receive_message().await?.try_into()?;
        // dbg!(&message3);

        let state5 = state4b.receive(message3);

        state5.redeem_btc(bitcoin_wallet).await.unwrap();

        Ok(alice::State::from(state5))
    })
}

//
// pub fn run_alice(state0: alice::State0, mut node: AliceNode) -> Gen<(alice::State, AliceNode), (), impl Future<Output = anyhow::Result<alice::State5>>> {
//     Gen::new(|co| async move {
//         node.transport.send_message(state0.next_message(&mut OsRng).into()).await?;
//         let bob_message0: bob::Message0 = node.transport.receive_message().await?.try_into()?;
//         let state1: alice::State1 = state0.receive(bob_message0)?;
//         co.yield_((alice::State::State1(state1), node)).await;
//
//         node.transport.send_message()
//         Err(anyhow!("bla"))
//     })
// }
//
// pub fn run_bob(state0: bob::State0, mut node: BobNode, ) -> Gen<(bob::State, BobNode), (), impl Future<Output = anyhow::Result<bob::State5>>> {
//     Gen::new(|co| async move {
//         node.transport.send_message(state0.next_message(&mut OsRng).into()).await?;
//         let alice_message0: alice::Message0 = node.transport.receive_message().await?.try_into()?;
//         let state1: bob::State1 = state0.receive(alice_message0)?;
//         co.yield_((alice::State::State1(state1), node)).await;
//
//         node.transport.send_message()
//         Err(anyhow!("bla"))
//     })
// }


async fn async_two() -> Result<i32> {
    Ok(2)
}

pub fn run_even() -> Gen<i32, (), impl Future<Output = anyhow::Result<i32>>> {
    Gen::new(|co| async move {
        let mut n = async_two().await?;
        while n < 100 {
            co.yield_(n).await;
            n += 2;
        }
        Ok(n)
    })
}

pub async fn run_even_until(even_number: i32) -> Result<i32> {
    let mut even = run_even();
    loop {
        match even.async_resume().await {
            GeneratorState::Yielded(i) => {
                if i == even_number {
                    return Ok(i)
                }
            }
            GeneratorState::Complete(r) => {
                return r
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::util::SubscriberInitExt;

    use crate::alice::node::run_even_until;

    #[tokio::test]
    async fn gen() {

        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();


        let (a, b) = futures::future::join(run_even_until(16), run_even_until(10)).await;
        tracing::info!("{:?}", a);
        tracing::info!("{:?}", b);
    }
}
