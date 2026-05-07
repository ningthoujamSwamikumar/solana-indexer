use serde::Serialize;
use solana_client::rpc_request::Address;
use sqlx::prelude::FromRow;

#[derive(FromRow, Serialize)]
pub struct Block {
    pub slot: i64,
    pub blockhash: String,
    pub block_time: i64,
    pub parent_slot: i64,
}

#[derive(FromRow, Serialize)]
pub struct Transfer {
    pub from: Address,
    pub base: Option<Address>,
    pub to: Address,
    /// in lamports for SOL
    /// in decimal for other tokens
    pub amount: u64,
    pub mint: Option<Address>,
}
