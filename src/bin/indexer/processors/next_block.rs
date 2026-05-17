use anyhow::{Ok, Result};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{CommitmentConfig, RpcBlockConfig, UiTransactionEncoding},
    rpc_response::UiConfirmedBlock,
};
use sqlx::{Pool, Postgres};

use crate::processors::blocks::process_blocks;

/// fetches the next slot (next to the latest slot in db) and processed the block
pub async fn fetch_and_process_next_slot_block(
    pg_pool: &Pool<Postgres>,
    arc_rpc: &RpcClient,
) -> Result<()> {
    //fetch the next slot after the last commitment
    let last_committed_slot: i64 = sqlx::query_scalar("SELECT MAX(slot) FROM blocks;")
        .fetch_one(pg_pool)
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

    process_blocks(block, slot, pg_pool.clone(), &arc_rpc).await?;

    Ok(())
}
