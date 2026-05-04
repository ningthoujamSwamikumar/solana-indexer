use anyhow::{Ok, Result};
use base64::{Engine, engine::general_purpose};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{CommitmentConfig, RpcBlockConfig, UiTransactionEncoding},
    rpc_response::{
        EncodedTransaction, UiConfirmedBlock, transaction::versioned::VersionedTransaction,
    },
};
use sqlx::{PgPool, types::Json};

#[allow(dead_code)]
const MAINNET_URL: &str = "https://api.mainnet.solana.com";
#[allow(dead_code)]
const DEVNET_URL: &str = "https://api.devnet.solana.com";
#[allow(dead_code)]
const LOCALNET_URL: &str = "http://localhost:8899";

const DATABASE_URL: &str = "postgresql://postgres@localhost:5432/solana_index";

#[tokio::main]
async fn main() -> Result<()> {
    println!("Welcome to Solana Indexer!!");

    // connect to database
    let pg_pool = PgPool::connect(DATABASE_URL).await?;
    // create schemas
    sqlx::query("CREATE TABLE blocks (slot BIGINT PRIMARY KEY, blockhash TEXT, parent_slot BIGINT, block_time BIGINT);")
    .execute(&pg_pool).await?;
    println!("blocks table created");
    sqlx::query("CREATE TABLE transactions (signature TEXT PRIMARY KEY, slot BIGINT REFERENCES blocks(slot), tx_base64 TEXT NOT NULL, meta JSONB);")
    .execute(&pg_pool).await?;
    println!("transactions table created");

    let rpc_client =
        RpcClient::new_with_commitment(MAINNET_URL.to_string(), CommitmentConfig::finalized());

    let slot = rpc_client.get_slot().await?;
    println!("slot: {slot}");

    let UiConfirmedBlock {
        block_time,
        blockhash,
        transactions,
        parent_slot,
        previous_blockhash: _,
        signatures: _,
        rewards: _,
        num_reward_partitions: _,
        block_height: _,
    } = rpc_client
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

    sqlx::query(
        "INSERT INTO blocks (slot, blockhash, parent_slot, block_time)
            VALUES ($1, $2, $3, $4);",
    )
    .bind(slot as i64)
    .bind(blockhash)
    .bind(parent_slot as i64)
    .bind(block_time.unwrap())
    .execute(&pg_pool)
    .await?;

    for tx in transactions.unwrap_or(vec![]) {
        // Extract base64 string from the EncodedTransaction enum
        let EncodedTransaction::Binary(tx_base64, _) = tx.transaction else {
            continue; //Skip for some reason it's not base64
        };

        // Decode the base64 into raw bytes
        let tx_bytes = general_purpose::STANDARD.decode(&tx_base64)?;

        // Deserialized the bytes into VersionedTransaction
        let versioned_tx = bincode::deserialize::<VersionedTransaction>(tx_bytes.as_slice())?;

        let sig = versioned_tx
            .signatures
            .first()
            .map(|s| s.to_string())
            .unwrap_or_default();

        sqlx::query(
            "INSERT INTO transactions (signature, slot, tx_base64, meta)
                VALUES ($1, $2, $3, $4);",
        )
        .bind(sig)
        .bind(slot as i64)
        .bind(tx_base64)
        .bind(Json(tx.meta))
        .execute(&pg_pool)
        .await?;
    }

    Ok(())
}
