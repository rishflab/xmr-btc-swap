use anyhow::Result;
use swap::storage::Database;

use swap::cli::Options;

pub struct TxLock;

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
pub enum AliceState {
    Started,
    Negotiated,
    BtcLocked,
    XmrLocked,
    BtcRedeemed,
    XmrRefunded,
    Cancelled,
    Punished,
    SafelyAborted,
}

// This struct contains all the I/O required to execute a swap
pub struct Io {
    // swarm: libp2p::Swarm<>,
// bitcoind_rpc: _,
// monerod_rpc: _,
// monero_wallet_rpc: _,
// db: _,
}

// State machine driver for swap execution
pub async fn swap(state: AliceState, io: Io) -> Result<AliceState> {
    match state {
        AliceState::Started => {
            // Alice and Bob exchange swap info
            // Todo: Poll the swarm here until Alice and Bob have exchanged info
            swap(AliceState::Negotiated, io)
        }
        AliceState::Negotiated => {
            // Alice and Bob have exchanged info
            // Todo: Alice watches for BTC to be locked on chain
            // Todo: Timeout at t1?
            swap(AliceState::BtcLocked, io)
        }
        AliceState::BtcLocked => {
            // Alice has seen that Bob has locked BTC
            // Todo: Alice locks XMR
            swap(AliceState::XmrLocked, io)
        }
        AliceState::XmrLocked => {
            // Alice has locked Xmr
            // Alice waits until Bob sends her key to redeem BTC
            // Todo: Poll the swarm here until msg from Bob arrives or t1
            let key_received = unimplemented!();

            if key_received {
                // Alice redeems BTC
                swap(AliceState::BtcRedeemed, io)
            } else {
                // submit TxCancel
                swap(AliceState::Cancelled, io)
            }
        }
        AliceState::Cancelled => {
            // Wait until t2 or if TxRefund is seen
            // If Bob has refunded the Alice should extract Bob's monero secret key and move
            // the TxLockXmr output to her wallet.
            let refunded = unimplemented!();
            if refunded {
                swap(AliceState::XmrRefunded, io)
            } else {
                swap(AliceState::Punished, io)
            }
        }
        AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
        AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
        AliceState::Punished => Ok(AliceState::Punished),
        AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
    }
}

// State machine driver for recovery execution
pub async fn abort(state: AliceState, io: Io) -> Result<AliceState> {
    match state {
        AliceState::Started => {
            // Nothing has been commited by either party, abort swap.
            abort(AliceState::SafelyAborted, io)
        }
        AliceState::Negotiated => {
            // Nothing has been commited by either party, abort swap.
            abort(AliceState::SafelyAborted, io)
        }
        AliceState::BtcLocked => {
            // Alice has seen that Bob has locked BTC
            // Alice does not need to do anything to recover
            abort(AliceState::SafelyAborted, io)
        }
        AliceState::XmrLocked => {
            // Alice has locked XMR
            // Alice watches for TxRedeem until t1
            if unimplemented!("TxRedeemSeen") {
                // Alice has successfully redeemed, protocol was a success
                abort(AliceState::BtcRedeemed, io)
            } else if unimplemented!("T1Elapsed") {
                // publish TxCancel or see if it has been published
                abort(AliceState::Cancelled, io)
            }
        }
        AliceState::Cancelled => {
            // Alice has cancelled the swap
            // Alice waits watches for t2 or TxRefund
            if unimplemented!("TxRefundSeen") {
                // Bob has refunded and leaked s_b
                abort(AliceState::XmrRefunded, io)
            } else if unimplemented!("T1Elapsed") {
                // publish TxCancel or see if it has been published
                // Wait until t2 and publish TxPunish
                abort(AliceState::Punished, io)
            }
        }
        AliceState::BtcRedeemed => Ok(AliceState::BtcRedeemed),
        AliceState::XmrRefunded => Ok(AliceState::XmrRefunded),
        AliceState::Punished => Ok(AliceState::Punished),
        AliceState::SafelyAborted => Ok(AliceState::SafelyAborted),
    }
}

fn main() {
    let opt = Options::from_args();

    let io: Io = {
        let db = Database::open(std::path::Path::new("./.swap-db/")).unwrap();
        unimplemented!()
    };

    match opt {
        Options::Alice { .. } => swap(AliceState::Started, io),
        Options::Recover { .. } => {
            let stored_state: AliceState = unimplemented!("io.get_state(uuid)?");
            abort(stored_state, io);
        }
        _ => {}
    };
}
