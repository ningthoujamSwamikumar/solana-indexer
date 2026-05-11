use anyhow::{Ok, Result};
use base64::{Engine, engine::general_purpose};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{CommitmentConfig, RpcBlockConfig, UiTransactionEncoding},
    rpc_response::{
        EncodedTransaction,
        UiConfirmedBlock, //transaction::versioned::VersionedTransaction,
    },
};
use solana_indexer::daos::{AccountDao, TransferDao, TxnAccountDao};
use sqlx::{PgPool, types::Json};

use crate::backfill::{
    batch_insert_into_accounts, batch_insert_into_transaction_accounts,
    batch_insert_into_transfers, get_accounts_and_transfers_from_txn_message,
};

#[allow(dead_code)]
const MAINNET_URL: &str = "https://api.mainnet.solana.com";
#[allow(dead_code)]
const DEVNET_URL: &str = "https://api.devnet.solana.com";
#[allow(dead_code)]
const LOCALNET_URL: &str = "http://localhost:8899";

const DATABASE_URL: &str = "postgresql://postgres@localhost:5432/solana_index";

mod backfill;
mod processors;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Welcome to Solana Indexer!!");

    // connect to database
    let pg_pool = PgPool::connect(DATABASE_URL).await?;
    // create schemas
    // blocks table to store block infos
    sqlx::query("CREATE TABLE IF NOT EXISTS blocks (slot BIGINT PRIMARY KEY, blockhash TEXT, parent_slot BIGINT, block_time BIGINT);")
    .execute(&pg_pool).await?;
    println!("blocks table created");
    // transactions table to store raw transactions in a block
    sqlx::query("CREATE TABLE IF NOT EXISTS transactions (signature TEXT PRIMARY KEY, slot BIGINT REFERENCES blocks(slot), tx_base64 TEXT NOT NULL, meta JSONB);")
    .execute(&pg_pool).await?;
    println!("transactions table created");
    //accounts table to store all accounts seen in transactions
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (pubkey TEXT PRIMARY KEY, first_seen_slot BIGINT);",
    )
    .execute(&pg_pool)
    .await?;
    println!("accounts table created");
    // transfer table to store all SOL and spl transfers found in transactions
    // base account is for funding accounts whose authority is the base account
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS transfers 
        (txn_signature TEXT PRIMARY KEY REFERENCES transactions(signature),
        program_id TEXT NOT NULL,
        from_address TEXT REFERENCES accounts(pubkey),
        base_address TEXT REFERENCES accounts(pubkey),
        to_address TEXT REFERENCES accounts(pubkey),
        amount BIGINT,
        mint_address TEXT);",
    )
    .execute(&pg_pool)
    .await?;
    println!("transfers table created");
    // accounts in transactions - a composite table
    // stores all accounts used in every transaction along with their account meta information
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS transaction_accounts 
        (signature TEXT NOT NULL REFERENCES transactions(signature),
        account_pubkey TEXT NOT NULL REFERENCES accounts(pubkey),
        is_signer BOOL NOT NULL,
        is_writable BOOL NOT NULL,
        PRIMARY KEY (signature, account_pubkey));",
    )
    .execute(&pg_pool)
    .await?;
    println!("transaction_accounts table created");

    let rpc_client =
        RpcClient::new_with_commitment(MAINNET_URL.to_string(), CommitmentConfig::finalized());

    println!("================ BACKFILL STARTED ==================");
    backfill::backfill_transfers_accounts(&pg_pool, &rpc_client).await?;
    println!("================ BACKFILL COMPLETED ================");

    let slot = rpc_client.get_slot().await?;
    println!("\n**************** new slot: {slot} *************************");
    println!("**************** new slot: {slot} *************************\n");

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
        let versioned_tx = bincode::deserialize::<solana_sdk::transaction::VersionedTransaction>(
            tx_bytes.as_slice(),
        )?;

        let sig = versioned_tx
            .signatures
            .first()
            .map(|s| s.to_string())
            .unwrap_or_default();

        sqlx::query(
            "INSERT INTO transactions (signature, slot, tx_base64, meta)
                VALUES ($1, $2, $3, $4);",
        )
        .bind(sig.clone())
        .bind(slot as i64)
        .bind(tx_base64)
        .bind(Json(&tx.meta))
        .execute(&pg_pool)
        .await?;

        // extract accounts, and transfers from the txn
        let (accounts, transfers) = get_accounts_and_transfers_from_txn_message(
            versioned_tx.message,
            tx.meta,
            &rpc_client,
        )
        .await?;

        // batch insert the accounts
        let accounts_insertion =
            batch_insert_into_accounts(&accounts, slot as i64, &pg_pool).await?;
        println!(
            "inserted {} rows into accounts",
            accounts_insertion.rows_affected()
        );

        // batch insert the transfers
        let transfer_insertion = batch_insert_into_transfers(transfers, &sig, &pg_pool).await?;
        println!(
            "inserted {} rows into transfers",
            transfer_insertion.rows_affected()
        );

        // batch insert into transaction accounts
        let txn_acc_insertion =
            batch_insert_into_transaction_accounts(accounts, &sig, &pg_pool).await?;
        println!(
            "inserted {} rows into transaction_accounts",
            txn_acc_insertion.rows_affected()
        );
    }

    Ok(())
}
