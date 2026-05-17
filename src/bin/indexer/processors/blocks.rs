use anyhow::{Ok, Result};
use base64::{Engine, engine::general_purpose};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_response::{EncodedTransaction, UiConfirmedBlock},
};
use sqlx::{PgPool, types::Json};

use crate::processors::{
    accounts_and_transfers::get_accounts_and_transfers_from_txn_message,
    batch_inserts::{
        batch_insert_into_accounts, batch_insert_into_transaction_accounts,
        batch_insert_into_transfers,
    },
};

pub async fn process_blocks(
    block: UiConfirmedBlock,
    slot: u64,
    pg_pool: PgPool,
    rpc_client: &RpcClient,
) -> Result<()> {
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
    } = block;

    // start a transaction
    let mut pg_tx = pg_pool.begin().await?;

    sqlx::query(
        "INSERT INTO blocks (slot, blockhash, parent_slot, block_time)
            VALUES ($1, $2, $3, $4);",
    )
    .bind(slot as i64)
    .bind(blockhash)
    .bind(parent_slot as i64)
    .bind(block_time.unwrap())
    .execute(&mut *pg_tx)
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
        .execute(&mut *pg_tx)
        .await?;

        // extract accounts, and transfers from the txn
        let (accounts, transfers) =
            get_accounts_and_transfers_from_txn_message(versioned_tx.message, tx.meta, rpc_client)
                .await?;

        // batch insert the accounts
        let accounts_insertion =
            batch_insert_into_accounts(&accounts, slot as i64, &mut *pg_tx).await?;
        println!(
            "inserted {} rows into accounts",
            accounts_insertion.rows_affected()
        );

        // batch insert the transfers
        let transfer_insertion = batch_insert_into_transfers(transfers, &sig, &mut *pg_tx).await?;
        println!(
            "inserted {} rows into transfers",
            transfer_insertion.rows_affected()
        );

        // batch insert into transaction accounts
        let txn_acc_insertion =
            batch_insert_into_transaction_accounts(accounts, &sig, &mut *pg_tx).await?;
        println!(
            "inserted {} rows into transaction_accounts",
            txn_acc_insertion.rows_affected()
        );
    }

    pg_tx.commit().await?;

    Ok(())
}
