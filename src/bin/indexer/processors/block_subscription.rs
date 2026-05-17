use std::sync::Arc;

use futures::StreamExt;

use anyhow::{Ok, Result};
use solana_client::{
    nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient},
    rpc_config::{RpcBlockSubscribeConfig, UiTransactionEncoding},
    rpc_response::RpcBlockUpdate,
};
use sqlx::PgPool;
use tokio::sync::Semaphore;

use crate::processors::blocks::process_blocks;

pub async fn run_block_subscription(
    pg_pool: &PgPool,
    arc_rpc: &Arc<RpcClient>,
    ws_rpc_url: String,
) -> Result<()> {
    // subscribe to blocks through websockets
    let live_stream_worker_handle = tokio::spawn(handle_block_streams(
        pg_pool.clone(),
        arc_rpc.clone(),
        ws_rpc_url,
    ));
    live_stream_worker_handle.await??;

    Ok(())
}

/// Subscribes to websocket rpc, and recieves block streams
async fn handle_block_streams(
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
