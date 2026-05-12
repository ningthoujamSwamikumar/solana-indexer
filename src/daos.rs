use serde::Serialize;
use solana_client::rpc_response::UiTransactionStatusMeta;
use sqlx::{prelude::FromRow, types::Json};

#[derive(FromRow, Serialize)]
pub struct BlockDao {
    pub slot: i64,
    pub blockhash: String,
    pub block_time: i64,
    pub parent_slot: i64,
}

#[derive(FromRow, Serialize)]
pub struct TransactionDao {
    pub signature: String,
    pub slot: i64,
    pub tx_base64: String,
    pub meta: Json<Option<UiTransactionStatusMeta>>,
}

#[derive(FromRow, Serialize)]
pub struct AccountDao {
    pub pubkey: String,
    pub first_seen_slot: i64,
}

#[derive(FromRow, Serialize)]
pub struct TransferDao {
    pub txn_signature: String,
    pub program_id: String,
    pub from_address: String,
    pub base_address: Option<String>,
    pub to_address: String,
    /// in lamports for SOL
    /// in decimal for other tokens
    pub amount: i64,
    pub mint_address: Option<String>,
}

#[derive(FromRow, Serialize)]
pub struct TxnAccountDao {
    pub signature: String,
    pub account_pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}
