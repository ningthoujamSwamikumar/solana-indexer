use std::sync::Arc;

use futures::StreamExt;

use anyhow::{Ok, Result};
use solana_client::{
    nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient},
    rpc_config::{
        CommitmentConfig, RpcBlockConfig, RpcBlockSubscribeConfig, UiTransactionEncoding,
    },
    rpc_response::{RpcBlockUpdate, UiConfirmedBlock},
};
use sqlx::{PgPool, Pool, Postgres};
use tokio::sync::Semaphore;

use crate::processors::{
    block_subscription::run_block_subscription, blocks::process_blocks, dex::run_dex_subscription,
};

#[allow(dead_code)]
const MAINNET_URL: &str = "api.mainnet.solana.com";

const DATABASE_URL: &str = "postgresql://postgres@localhost:5432/solana_index";

mod backfill;
mod create_tables;
mod processors;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Welcome to Solana Indexer!!");

    // load the .env file into the process environments
    dotenv::dotenv().ok();

    // read env vars
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| format!("https://{}", MAINNET_URL));
    let ws_rpc_url =
        std::env::var("WEBSOCKET_RPC_URL").unwrap_or_else(|_| format!("wss://{}", MAINNET_URL));
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DATABASE_URL.to_string());

    // connect to database
    let pg_pool = PgPool::connect(&db_url).await?;
    create_tables::create_tables(&mut *pg_pool.acquire().await?).await?;

    let rpc_client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::finalized());
    let arc_rpc = Arc::new(rpc_client);

    run_dex_subscription(ws_rpc_url, &pg_pool).await?;

    Ok(())
}
