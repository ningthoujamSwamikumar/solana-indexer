use anyhow::{Ok, Result};

use sqlx::{Execute, PgConnection, Postgres, QueryBuilder, postgres::PgQueryResult};

use crate::processors::{Transfer, TxnAccount};

pub async fn batch_insert_into_accounts(
    accounts: &Vec<TxnAccount>,
    slot: i64,
    executor: &mut PgConnection,
) -> Result<PgQueryResult> {
    // the trailing space in the initial sql fragment is important to not get errors, and as query builder doesn't automatically appends it
    let mut accounts_qb: QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("INSERT INTO accounts (pubkey, first_seen_slot) ");
    accounts_qb.push_values(
        accounts.iter().map(|account| account.0),
        |mut b: sqlx::query_builder::Separated<'_, '_, Postgres, &'static str>, pubkey| {
            b.push_bind(pubkey.to_string()).push_bind(slot);
        },
    );
    // appends conflict handling
    accounts_qb.push(" ON CONFLICT DO NOTHING;");
    let account_insertion = accounts_qb.build().execute(executor).await?;

    Ok(account_insertion)
}

pub async fn batch_insert_into_transfers(
    transfers: Vec<Transfer>,
    sig: &str,
    executor: &mut PgConnection,
) -> Result<PgQueryResult> {
    if transfers.is_empty() {
        println!("Found empty transfers! Running no op query.");
        let empty_query = sqlx::query("SELECT 1;").execute(executor).await?;
        return Ok(empty_query);
    }

    let mut transfers_qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO transfers (txn_signature, program_id, from_address, base_address, to_address, amount, mint_address) ",
    );
    transfers_qb.push_values(
        transfers.into_iter(),
        |mut b, (program_id, from_address, base_address, to_address, amount, mint_address): (_, _, Option<_>, _, u64, Option<_>)| {
            if sig == "2PUSFcg7eVXKsa3mrKAiJLg48VGjBWn7s1Y5GzZQutjGFCyxZz72qH4RyGs9bMNxZnTF9JhAg2dgp41uGX7KKnER" {
                println!("values bindings which causes constraint violation:\nprogram_id: {}\nfrom_address: {}\nbase_address: {:?}\nto_address: {}\namount: {}\nmint_address: {:?}", program_id.to_string(), from_address.to_string(), base_address, to_address.to_string(), amount, mint_address);
            }

            b.push_bind(sig)
                .push_bind(program_id.to_string())
                .push_bind(from_address.to_string())
                .push_bind(base_address.map(|address| address.to_string()))
                .push_bind(to_address.to_string())
                .push_bind(amount as i64)
                .push_bind(mint_address.map(|address| address.to_string()));
        },
    );
    transfers_qb.push(" ON CONFLICT DO NOTHING;");

    let transfer_query = transfers_qb.build();

    if sig
        == "2PUSFcg7eVXKsa3mrKAiJLg48VGjBWn7s1Y5GzZQutjGFCyxZz72qH4RyGs9bMNxZnTF9JhAg2dgp41uGX7KKnER"
    {
        println!(
            "transfer_query with constraint violation:\n{}",
            transfer_query.sql()
        );
    }

    let transfer_insertions = transfer_query.execute(executor).await?;

    Ok(transfer_insertions)
}

pub async fn batch_insert_into_transaction_accounts(
    accounts: Vec<TxnAccount>,
    sig: &str,
    executor: &mut PgConnection,
) -> Result<PgQueryResult> {
    let mut txn_acc_qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "INSERT INTO transaction_accounts (signature, account_pubkey, is_signer, is_writable) ",
    );
    txn_acc_qb.push_values(
        accounts.into_iter(),
        |mut b, (pubkey, is_signer, is_writable)| {
            b.push_bind(sig)
                .push_bind(pubkey.to_string())
                .push_bind(is_signer)
                .push_bind(is_writable);
        },
    );
    txn_acc_qb.push(" ON CONFLICT DO NOTHING;");
    let txn_acc_insertions = txn_acc_qb.build().execute(executor).await?;

    Ok(txn_acc_insertions)
}
