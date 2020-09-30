use crate::{alice, bob, SendReceive, Transport};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use genawaiter::sync::{Co, Gen};
use std::{
    future::Future,
    future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::stream::Stream;
#[derive(Debug)]
pub struct AliceNode {
    transport: Transport<alice::Message, bob::Message>,
    state: alice::State,
}
use genawaiter::{yield_, GeneratorState};

async fn async_one() -> i32 {
    1
}
async fn async_two() -> i32 {
    2
}

pub fn run_even() -> Gen<i32, (), impl Future<Output = ()>> {
    let a = Gen::new(|co| async move {
        let mut n = async_two().await;
        while n < 100 {
            co.yield_(n).await;
            n += 2;
        }
    });
    a
}

pub async fn run_even_until(even_number: i32) -> Result<i32> {
    let mut even = run_even();
    while let GeneratorState::Yielded(i)  = even.async_resume().await {
        if i == even_number {
            return Ok(i)
        } else {
            tracing::info!("{}", i);
        }
    }
    return Err(anyhow!("WEfw"))
}


pub fn run_odd() -> Gen<i32, (), impl Future<Output = ()>> {
    let a = Gen::new(|co| async move {
        let mut n = async_one().await;
        while n < 100 {
            co.yield_(n).await;
            n += 2;
        }
    });
    a
}



#[cfg(test)]
mod tests {
    use tracing_subscriber::util::SubscriberInitExt;
    use genawaiter::GeneratorState;
    use crate::alice::node::run_even_until;

    #[tokio::test]
    async fn gen() {

        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();

        let mut even = super::run_even();
        let mut odd = super::run_odd();

     

        futures::future::join(run_even_until(16), run_even_until(10)).await;
    }
}
