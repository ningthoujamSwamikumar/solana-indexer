use anyhow::{Ok, Result};
use base64::Engine;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_indexer::daos::TransactionDao;
use solana_sdk::transaction::VersionedTransaction;
use sqlx::{PgConnection, types::Json};

use crate::processors::{
    accounts_and_transfers::get_accounts_and_transfers_from_txn_message,
    batch_inserts::{
        batch_insert_into_accounts, batch_insert_into_transaction_accounts,
        batch_insert_into_transfers,
    },
};

/// Find all the transactions present in the transactions table and fill their corresponding entries into new tables created <br/>
/// like accounts, transfers, and transaction accounts
pub async fn backfill_transfers_accounts(
    executor: &mut PgConnection,
    rpc: &RpcClient,
) -> Result<()> {
    /*
    let accounts: Vec<AccountDao> = sqlx::query_as("SELECT * from accounts;")
        .fetch_all(pg_pool)
        .await?;
    let transfers: Vec<TransferDao> = sqlx::query_as("SELECT * FROM transfers;")
        .fetch_all(pg_pool)
        .await?;
    let txn_accounts: Vec<TxnAccountDao> = sqlx::query_as("SELECT * FROM transaction_accounts;")
        .fetch_all(pg_pool)
        .await?;

    if !accounts.is_empty() && !transfers.is_empty() && !txn_accounts.is_empty() {
        return Ok(());
    }
    */

    //go through all recorded transactions
    let txns: Vec<TransactionDao> =
        sqlx::query_as("SELECT signature, slot, tx_base64, meta FROM transactions;")
            .fetch_all(&mut *executor)
            .await?;
    for TransactionDao {
        signature,
        slot,
        tx_base64,
        meta: Json(txn_meta),
    } in txns
    {
        println!(
            "*********** processing txn: {} for slot: {} ************",
            signature, slot
        );

        //extract programs, accounts, datas
        let tx_bytes = base64::engine::general_purpose::STANDARD.decode(&tx_base64)?;
        let txn: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
        let (accounts, transfers) =
            get_accounts_and_transfers_from_txn_message(txn.message, txn_meta, rpc).await?;

        //insert accounts into accounts
        let account_insertion = batch_insert_into_accounts(&accounts, slot, executor).await?;
        println!(
            "{} rows inserted into accounts for slot {}",
            account_insertion.rows_affected(),
            slot
        );
        // DEBUG
        if signature
            == "2PUSFcg7eVXKsa3mrKAiJLg48VGjBWn7s1Y5GzZQutjGFCyxZz72qH4RyGs9bMNxZnTF9JhAg2dgp41uGX7KKnER"
        {
            println!("all accounts: \n{:?}", accounts);

            let debug_finding = accounts
                .iter()
                .find(|(a, _, _)| a.to_string() == "67YBbzcj2EpeeejjouJrsYWq6fGtzXZPCPm6br2a3duY");
            if debug_finding.is_none() {
                println!("The target address couldn't found in accounts list");
            } else {
                println!("The target address is found in the accounts list for the target txn");
            }
        }
        // DEBUG END

        //insert transfers into transfers
        let transfer_insertions =
            batch_insert_into_transfers(transfers, &signature, executor).await?;
        println!(
            "{} rows inserted into transfers for slot {} and for txn {}",
            transfer_insertions.rows_affected(),
            slot,
            signature
        );

        //insert transaction accounts into transaction_accounts
        let txn_acc_insertions =
            batch_insert_into_transaction_accounts(accounts, &signature, executor).await?;
        println!(
            "{} rows inserted into transaction_accounts for slot {} and txn {}",
            txn_acc_insertions.rows_affected(),
            slot,
            signature
        );
    }

    Ok(())
}
