use std::{str::FromStr, sync::Arc};

use anyhow::{Ok, Result};
use base64::Engine;
use solana_client::{
    nonblocking::pubsub_client::PubsubClient,
    rpc_config::{RpcAccountInfoConfig, UiAccountEncoding},
    rpc_response::{UiAccount, UiAccountData},
};
use solana_sdk::pubkey::Pubkey;

use futures::StreamExt;
use sqlx::PgPool;

/// Whirpool account for SOL/USDC pool
pub const ORCA_POOL: &str = "HJPjoWUrhoZzkNfRpHuieeFk9WcZWjwy6PBjZ81ngndJ"; //"7qbRF6YsyGuLUVs6Y1q64bdVrfe4ZcUUz1JRdoVNuJ3E";

pub async fn run_dex_subscription(ws_url: String, pg_pool: &PgPool) -> Result<()> {
    println!("Welcome to Orca Subscription.");
    let orca_subsciption = tokio::spawn(establish_connection(ws_url, pg_pool.clone()));
    orca_subsciption.await??;

    Ok(())
}

async fn establish_connection(ws_url: String, pg_pool: PgPool) -> Result<()> {
    let orca_pool_pubkey: Pubkey = Pubkey::from_str(ORCA_POOL)?;
    let ws_client = PubsubClient::new(ws_url).await?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<UiAccount>(50);
    let semaphor = Arc::new(tokio::sync::Semaphore::new(pg_pool.size() as usize));

    // dispatcher
    // run this dispatcher in the background, so, it doesn't block the main thread
    tokio::spawn(async move {
        while let Some(ui_account) = rx.recv().await {
            println!(
                "account received from channel. Remaining {} accounts in the channel",
                rx.len()
            );
            let permit = semaphor.clone().acquire_owned().await.unwrap();

            // spawn workers
            tokio::spawn(async move {
                println!("Processing account...");
                //process accounts
                process_account(ui_account).await.unwrap();

                //release permit
                drop(permit);
            });
        }
    });

    // subscribe to accounts
    let (mut account_stream, _unsubscribe) = ws_client
        .account_subscribe(
            &orca_pool_pubkey,
            Some(RpcAccountInfoConfig {
                encoding: Some(solana_client::rpc_config::UiAccountEncoding::Base64),
                ..Default::default()
            }),
        )
        .await?;

    println!("websocket connection established");

    while let Some(response) = account_stream.next().await {
        println!("Recieved Ui account response.");
        let account = response.value;
        tx.send(account).await?;
    }

    Ok(())
}

// process the pool account
async fn process_account(account: UiAccount) -> Result<()> {
    if let UiAccountData::Binary(base64_data, UiAccountEncoding::Base64) = account.data {
        let raw_bytes = base64::engine::general_purpose::STANDARD.decode(base64_data)?;
        //decode to account structure
        // Whirlpool account:
        // Bytes 0-8: discriminator (Anchor's unique tag)
        // Bytes 8-40: whirlpools_config (Pubkey)
        // Bytes 40-41: whirlpool_bump (u8)
        // Bytes 41-43: tick_spacing (u16)
        // Bytes 43-45: tick_spacing_seed (u16)
        // Bytes 45-47: fee_rate (u16)
        // Bytes 47-49: protocol_fee_rate (u16)
        // Bytes 49-65: liquidity (u128)
        // Bytes 65-81: sqrt_price (u128)
        let sqrt_price_bytes = &raw_bytes[65..81];

        // convert little-endian bytes into a Rust u128 integer
        let sqrt_price_x64 = u128::from_le_bytes(sqrt_price_bytes.try_into().unwrap());

        // Do the Q64.64 math to find the raw price
        // Orca stores price shifted by 2^64 to preserve precision
        let sqrt_price = sqrt_price_x64 as f64 / (1u128 << 64) as f64;
        let raw_price = sqrt_price * sqrt_price;

        // adjust the token decimals
        // To get the price of 1 SOL in USDC, we multiply by (10^9/10^6)
        let sol_decimal = 9; // SOL
        let usdc_decimal = 6; // USDC 
        let decimal_adjustment = 10f64.powi(sol_decimal - usdc_decimal);

        let human_price = raw_price * decimal_adjustment;

        println!("🔥 Trade! New SOL/USDC Price: ${:.4}", human_price);
    }

    Ok(())
}
