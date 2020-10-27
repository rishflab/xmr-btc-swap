use anyhow::Result;
use futures::{channel::mpsc, StreamExt};
use libp2p::core::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display},
    io,
    io::Write,
    process,
};
use tracing::info;
use xmr_btc::bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock};

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod config;
pub mod network;
pub mod storage;
#[cfg(feature = "tor")]
pub mod tor;

pub const ONE_BTC: u64 = 100_000_000;

const REFUND_TIMELOCK: u32 = 10; // Relative timelock, this is number of blocks. TODO: What should it be?
const PUNISH_TIMELOCK: u32 = 20; // FIXME: What should this be?

pub type Never = std::convert::Infallible;

/// Commands sent from Bob to the main task.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    VerifyAmounts(SwapAmounts),
}

/// Responses sent from the main task back to Bob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rsp {
    VerifiedAmounts,
    Abort,
}

/// XMR/BTC swap amounts.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SwapAmounts {
    /// Amount of BTC to swap.
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: ::bitcoin::Amount,
    /// Amount of XMR to swap.
    #[serde(with = "xmr_btc::serde::monero_amount")]
    pub xmr: xmr_btc::monero::Amount,
}

// TODO: Display in XMR and BTC (not picos and sats).
impl Display for SwapAmounts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} sats for {} piconeros",
            self.btc.as_sat(),
            self.xmr.as_piconero()
        )
    }
}

pub async fn swap_as_alice(
    addr: Multiaddr,
    redeem: ::bitcoin::Address,
    punish: ::bitcoin::Address,
) -> Result<()> {
    alice::swap(addr, redeem, punish).await
}

pub async fn swap_as_bob<W>(
    sats: u64,
    alice: Multiaddr,
    refund: ::bitcoin::Address,
    wallet: W,
) -> Result<()>
where
    W: BuildTxLockPsbt + SignTxLock + BroadcastSignedTransaction + Send + Sync + 'static,
{
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(sats, alice, cmd_tx, rsp_rx, refund, wallet));

    loop {
        let read = cmd_rx.next().await;
        match read {
            Some(cmd) => match cmd {
                Cmd::VerifyAmounts(p) => {
                    let rsp = verify(p);
                    rsp_tx.try_send(rsp)?;
                    if rsp == Rsp::Abort {
                        process::exit(0);
                    }
                }
            },
            None => {
                info!("Channel closed from other end");
                return Ok(());
            }
        }
    }
}

fn verify(amounts: SwapAmounts) -> Rsp {
    let mut s = String::new();
    println!("Got rate from Alice for XMR/BTC swap\n");
    println!("{}", amounts);
    print!("Would you like to continue with this swap [y/N]: ");

    let _ = io::stdout().flush();
    io::stdin()
        .read_line(&mut s)
        .expect("Did not enter a correct string");

    if let Some('\n') = s.chars().next_back() {
        s.pop();
    }
    if let Some('\r') = s.chars().next_back() {
        s.pop();
    }

    if !is_yes(&s) {
        println!("No worries, try again later - Alice updates her rate regularly");
        return Rsp::Abort;
    }

    Rsp::VerifiedAmounts
}

fn is_yes(s: &str) -> bool {
    matches!(s, "y" | "Y" | "yes" | "YES" | "Yes")
}
