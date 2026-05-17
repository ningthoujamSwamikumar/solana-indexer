use anyhow::{Ok, Result};
use sqlx::PgConnection;

/// create tables required for the indexer
pub async fn create_tables(db_conn: &mut PgConnection) -> Result<()> {
    // create schemas
    // blocks table to store block infos
    sqlx::query("CREATE TABLE IF NOT EXISTS blocks (slot BIGINT PRIMARY KEY, blockhash TEXT, parent_slot BIGINT, block_time BIGINT);")
    .execute(&mut *db_conn).await?;
    println!("blocks table created");

    // transactions table to store raw transactions in a block
    sqlx::query("CREATE TABLE IF NOT EXISTS transactions (signature TEXT PRIMARY KEY, slot BIGINT REFERENCES blocks(slot), tx_base64 TEXT NOT NULL, meta JSONB);")
    .execute(&mut *db_conn).await?;
    println!("transactions table created");

    //accounts table to store all accounts seen in transactions
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (pubkey TEXT PRIMARY KEY, first_seen_slot BIGINT);",
    )
    .execute(&mut *db_conn)
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
    .execute(&mut *db_conn)
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
    .execute(&mut *db_conn)
    .await?;
    println!("transaction_accounts table created");

    Ok(())
}
