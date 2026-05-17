use std::sync::Arc;

use futures::StreamExt;

use anyhow::{Ok, Result};
use solana_client::{
    nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient},
    rpc_config::{
        CommitmentConfig, RpcBlockConfig, RpcBlockSubscribeConfig, RpcTransactionLogsConfig,
        UiTransactionEncoding,
    },
    rpc_response::{RpcBlockUpdate, UiConfirmedBlock},
};
use sqlx::{PgPool, Postgres};
use tokio::sync::Semaphore;

use crate::processors::blocks::process_blocks;

#[allow(dead_code)]
const MAINNET_URL: &str = "api.mainnet.solana.com";
#[allow(dead_code)]
const DEVNET_URL: &str = "https://api.devnet.solana.com";
#[allow(dead_code)]
const LOCALNET_URL: &str = "http://localhost:8899";

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

    // connect to database
    let pg_pool = PgPool::connect(DATABASE_URL).await?;
    create_tables::create_tables(&mut *pg_pool.acquire().await?).await?;

    let rpc_client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::finalized());
    let arc_rpc = Arc::new(rpc_client);

    // backfill new tables: accounts, transaction_accounts, transfers from the already existing block and transactions tables
    // backfill::backfill_transfers_accounts(&pg_pool, &rpc_client).await?;

    // subscribe to blocks through websockets
    // let live_stream_worker_handle = tokio::spawn(handle_log_streams(
    //     pg_pool.clone(),
    //     arc_rpc.clone(),
    //     ws_rpc_url,
    // ));
    // live_stream_worker_handle.await??;

    //fetch the next slot after the last commitment
    let last_committed_slot: i64 = sqlx::query_scalar("SELECT MAX(slot) FROM blocks;")
        .fetch_one(&pg_pool)
        .await?;
    println!("Last committed slot: {}", last_committed_slot);

    let mut slot: u64 = arc_rpc.get_slot().await?;
    println!("\n**************** new slot: {slot} *************************");

    if last_committed_slot > 0 {
        slot = (last_committed_slot + 1) as u64;
        println!("backfilling slot : {slot}");
    }

    let block: UiConfirmedBlock = arc_rpc
        .get_block_with_config(
            slot,
            RpcBlockConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
                encoding: Some(UiTransactionEncoding::Base64),
                ..Default::default()
            },
        )
        .await?;

    process_blocks(block, slot, pg_pool, &arc_rpc).await?;

    Ok(())
}

/// Subscribes to websocket rpc, and recieves block streams
async fn handle_log_streams(
    pg_pool: PgPool,
    arc_rpc: Arc<RpcClient>,
    ws_rpc_url: String,
) -> Result<()> {
    println!("Block Live Stream task");
    // create a message channel to communicate between the websocket handle and the workers
    // this will handle the backpressure from fast incoming messages from websocket
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RpcBlockUpdate>(100);

    // Semaphor limits the pool size or number of workers
    let semaphor = Arc::new(Semaphore::new(pg_pool.size() as usize));
    let dispatcher_pg_pool = pg_pool.clone();
    let dispatcher_arc_rpc = arc_rpc.clone();
    // Receiver + Dispatcher + Permit handler (like a bouncer in a club)
    tokio::spawn(async move {
        while let Some(RpcBlockUpdate {
            slot,
            block,
            err: _,
        }) = rx.recv().await
        {
            // wait until a worker slot is available
            let permit = semaphor.clone().acquire_owned().await.unwrap();
            // clone the pointers
            let task_pg_pool = dispatcher_pg_pool.clone();
            let task_rpc_client = dispatcher_arc_rpc.clone();

            // spawn new worker
            tokio::spawn(async move {
                if let Some(block) = block {
                    process_blocks(block, slot, task_pg_pool, &task_rpc_client)
                        .await
                        .unwrap();
                }
                // permit drops here, opening up a slot for the next block processor
                drop(permit);
            });
        }
    });

    // establish websocket connection and listen to logs
    let pubsub_client = PubsubClient::new(ws_rpc_url).await?;
    let (mut block_notifications, _unsubscribe_blocks) = pubsub_client
        .block_subscribe(
            solana_client::rpc_config::RpcBlockSubscribeFilter::All,
            Some(RpcBlockSubscribeConfig {
                transaction_details: Some(solana_client::rpc_config::TransactionDetails::Full),
                encoding: Some(UiTransactionEncoding::Base64),
                ..Default::default()
            }),
        )
        .await?;

    // handle the incoming websocket messages
    while let Some(block_response) = block_notifications.next().await {
        tx.send(block_response.value).await?;
    }

    Ok(())
}
