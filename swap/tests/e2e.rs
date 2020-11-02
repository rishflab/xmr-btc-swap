#[cfg(feature = "tor")]
mod e2e_test {
    use bitcoin_harness::Bitcoind;
    use futures::{channel::mpsc, future::try_join};
    use libp2p::Multiaddr;
    use monero_harness::Monero;
    use std::{fs, sync::Arc};
    use swap::{alice, bob, tor::UnauthenticatedConnection};
    use tempfile::{Builder, NamedTempFile};
    use testcontainers::clients::Cli;
    use torut::utils::{run_tor, AutoKillChild};
    use tracing_subscriber::util::SubscriberInitExt;

    fn run_tmp_tor() -> anyhow::Result<(AutoKillChild, u16, u16, NamedTempFile)> {
        // we create an empty torrc file to not use the system one
        let temp_torrc = Builder::new().tempfile()?;
        let torrc_file = format!("{}", fs::canonicalize(temp_torrc.path())?.display());
        tracing::info!("Temp torrc file created at: {}", torrc_file);

        let control_port = if port_check::is_local_port_free(9051) {
            9051
        } else {
            port_check::free_local_port().unwrap()
        };
        let proxy_port = if port_check::is_local_port_free(9050) {
            9050
        } else {
            port_check::free_local_port().unwrap()
        };

        let child = run_tor(
            "tor",
            &mut [
                "--CookieAuthentication",
                "1",
                "--ControlPort",
                control_port.to_string().as_str(),
                "--SocksPort",
                proxy_port.to_string().as_str(),
                "-f",
                &torrc_file,
            ]
            .iter(),
        )?;
        tracing::info!("Tor running with pid: {}", child.id());
        let child = AutoKillChild::new(child);
        Ok((child, control_port, proxy_port, temp_torrc))
    }

    #[tokio::test]
    async fn swap() {
        let _guard = tracing_subscriber::fmt()
        .with_env_filter(
            "swap=debug,xmr_btc=debug,hyper=off,reqwest=off,monero_harness=info,testcontainers=info,libp2p=debug",
        )
        .with_ansi(false)
        .set_default();

        let (_child, control_port, proxy_port, _tmp_torrc) = run_tmp_tor().unwrap();

        let (alice_multiaddr, _ac): (Multiaddr, swap::tor::AuthenticatedConnection) = {
            let tor_secret_key = torut::onion::TorSecretKeyV3::generate();
            let onion_address = tor_secret_key
                .public()
                .get_onion_address()
                .get_address_without_dot_onion();
            (
                format!("/onion3/{}:{}", onion_address, 9877)
                    .parse()
                    .expect("failed to parse Alice's address"),
                {
                    let mut authenticated_connection =
                        UnauthenticatedConnection::with_ports(proxy_port, control_port)
                            .init_authenticated_connection()
                            .await
                            .unwrap();
                    authenticated_connection
                        .add_service(9877, &tor_secret_key)
                        .await
                        .unwrap();

                    authenticated_connection
                },
            )
        };

        let cli = Cli::default();
        let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
        let _ = bitcoind.init(5).await;

        let btc = bitcoin::Amount::from_sat(1_000_000);
        let btc_alice = bitcoin::Amount::ZERO;
        let btc_bob = btc * 10;

        // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
        // 10_000 * 100
        let xmr = 1_000_000_000_000;
        let xmr_alice = xmr * 10;
        let xmr_bob = 0;

        let alice_btc_wallet = Arc::new(
            swap::bitcoin::Wallet::new("alice", &bitcoind.node_url)
                .await
                .unwrap(),
        );
        let bob_btc_wallet = Arc::new(
            swap::bitcoin::Wallet::new("bob", &bitcoind.node_url)
                .await
                .unwrap(),
        );
        bitcoind
            .mint(bob_btc_wallet.0.new_address().await.unwrap(), btc_bob)
            .await
            .unwrap();

        let (monero, _container) = Monero::new(&cli).unwrap();
        monero.init(xmr_alice, xmr_bob).await.unwrap();

        let alice_xmr_wallet = Arc::new(swap::monero::Wallet(monero.alice_wallet_rpc_client()));
        let bob_xmr_wallet = Arc::new(swap::monero::Wallet(monero.bob_wallet_rpc_client()));

        let alice_swap = alice::swap(
            alice_btc_wallet.clone(),
            alice_xmr_wallet.clone(),
            alice_multiaddr.clone(),
            Some(9876),
        );

        let (cmd_tx, mut _cmd_rx) = mpsc::channel(1);
        let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
        let bob_swap = bob::swap(
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            btc.as_sat(),
            alice_multiaddr,
            cmd_tx,
            rsp_rx,
        );

        // automate the verification step by accepting any amounts sent over by Alice
        rsp_tx.try_send(swap::Rsp::VerifiedAmounts).unwrap();

        try_join(alice_swap, bob_swap).await.unwrap();

        let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
        let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

        let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();

        monero.wait_for_bob_wallet_block_height().await.unwrap();
        let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();

        assert_eq!(
            btc_alice_final,
            btc_alice + btc - bitcoin::Amount::from_sat(xmr_btc::bitcoin::TX_FEE)
        );
        assert!(btc_bob_final <= btc_bob - btc);

        assert!(xmr_alice_final.as_piconero() <= xmr_alice - xmr);
        assert_eq!(xmr_bob_final.as_piconero(), xmr_bob + xmr);
    }
}
