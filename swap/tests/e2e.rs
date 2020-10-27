mod tests {

    use config::Config;
    use libp2p::multiaddr::Multiaddr;
    use swap::{bitcoin::Wallet, config};

    #[tokio::test]
    async fn happy_path() {
        let config = Config::default();

        let alice: Multiaddr = config.listen_addr;
        let url = config.bitcoind_url;

        let sats_to_swap = 100;

        let bitcoin_wallet = Wallet::new("alice", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let redeem = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new redeem address");
        let punish = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new punish address");

        let alice_fut = swap::swap_as_alice(alice.clone(), redeem, punish);

        let bitcoin_wallet = Wallet::new("bob", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let refund = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new address");

        let bob_fut = swap::swap_as_bob(sats_to_swap, alice, refund, bitcoin_wallet);

        let (alice, bob) = tokio::join!(alice_fut, bob_fut);

        assert!(alice.is_ok());
        assert!(bob.is_ok());
    }
}
