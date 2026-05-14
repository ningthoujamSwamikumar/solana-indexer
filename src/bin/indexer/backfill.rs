use anyhow::{Ok, Result};
use base64::Engine;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_request::Address,
    rpc_response::{
        OptionSerializer, UiLoadedAddresses, UiTransactionStatusMeta, UiTransactionTokenBalance,
    },
};
use solana_indexer::daos::TransactionDao;
use solana_sdk::{
    message::VersionedMessage, program_pack::Pack, transaction::VersionedTransaction,
};
use solana_system_interface::{instruction::SystemInstruction, program::ID as system_program_id};

use spl_token_2022_interface::{
    instruction::TokenInstruction as TokenInstruction2022, state::Account as Account2022,
};
use spl_token_interface::{instruction::TokenInstruction, state::Account};

use sqlx::{
    Execute, PgConnection, Postgres, QueryBuilder, postgres::PgQueryResult,
    types::Json,
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

pub type TxnAccount = (Address, bool, bool);
///
/// Transfer (
///     program_id,
///     from_address,
///     base_address,
///     to_address,
///     amount,
///     mint_address
/// )
///
pub type Transfer = (
    Address,
    Address,
    Option<Address>,
    Address,
    u64,
    Option<Address>,
);

pub type Extraction = (Vec<TxnAccount>, Vec<Transfer>);

pub async fn get_accounts_and_transfers_from_txn_message(
    txn_message: VersionedMessage,
    txn_meta: Option<UiTransactionStatusMeta>,
    rpc: &RpcClient,
) -> Result<Extraction> {
    let extractions = match txn_message {
        VersionedMessage::Legacy(solana_sdk::message::Message {
            header,
            account_keys,
            recent_blockhash: _,
            instructions,
        }) => {
            println!("VersionedMessage::Legacy");
            println!("account_keys len: {}", account_keys.len());
            extract_accounts_and_transfers(header, None, account_keys, instructions, txn_meta, rpc)
                .await?
        }
        VersionedMessage::V0(solana_sdk::message::v0::Message {
            header,
            account_keys,
            recent_blockhash: _,
            instructions,
            address_table_lookups: _,
        }) => {
            println!("VersionedMessage::V0");

            let mut resolved_accounts_keys: Vec<Address> = vec![];
            resolved_accounts_keys.extend(account_keys);

            println!(
                "resolved_accounts_keys len: {}",
                resolved_accounts_keys.len()
            );

            let mut loaded_addresses: Option<(u8, u8)> = None;

            if let Some(meta) = &txn_meta {
                println!(
                    "txn_meta is Some, but does meta.loaded_addresses contain Some? {} or Skip? {} or None? {}",
                    meta.loaded_addresses.is_some(),
                    meta.loaded_addresses.is_skip(),
                    meta.loaded_addresses.is_none()
                );

                if let OptionSerializer::Some(UiLoadedAddresses { writable, readonly }) =
                    &meta.loaded_addresses
                {
                    resolved_accounts_keys
                        .extend(writable.iter().map(|addr| Address::from_str_const(addr)));
                    resolved_accounts_keys
                        .extend(readonly.iter().map(|addr| Address::from_str_const(addr)));

                    println!(
                        "after extension, resolved_accounts_keys len: {}",
                        resolved_accounts_keys.len()
                    );

                    loaded_addresses = Some((writable.len() as u8, readonly.len() as u8));
                }
            }

            extract_accounts_and_transfers(
                header,
                loaded_addresses,
                resolved_accounts_keys,
                instructions,
                txn_meta,
                rpc,
            )
            .await?
        }
        VersionedMessage::V1(solana_sdk::message::v1::Message {
            header,
            config: _,
            lifetime_specifier: _,
            account_keys,
            instructions,
        }) => {
            println!("VersionedMessage::V1");

            let mut resolved_accounts_keys: Vec<Address> = vec![];
            resolved_accounts_keys.extend(account_keys);

            println!(
                "resolved_accounts_keys len: {}",
                resolved_accounts_keys.len()
            );

            let mut loaded_addresses: Option<(u8, u8)> = None;

            if let Some(meta) = &txn_meta {
                if let OptionSerializer::Some(UiLoadedAddresses { writable, readonly }) =
                    &meta.loaded_addresses
                {
                    resolved_accounts_keys
                        .extend(writable.iter().map(|addr| Address::from_str_const(addr)));
                    resolved_accounts_keys
                        .extend(readonly.iter().map(|addr| Address::from_str_const(addr)));

                    println!(
                        "after extension, resolved_accounts_keys len: {}",
                        resolved_accounts_keys.len()
                    );

                    loaded_addresses = Some((writable.len() as u8, readonly.len() as u8));
                }
            }

            extract_accounts_and_transfers(
                header,
                loaded_addresses,
                resolved_accounts_keys,
                instructions,
                txn_meta,
                rpc,
            )
            .await?
        }
    };

    Ok(extractions)
}

/// Get accounts, and transfers from `VersionedMessage` components
/// accounts: [(address, is_signer, is_writable)]
/// transfers: [(program_id, from_wallet, optional(base_wallet), to_wallet, amount, optional(mint))]
pub async fn extract_accounts_and_transfers(
    header: solana_sdk::message::MessageHeader,
    loaded_addresses: Option<(u8, u8)>, //(num_writable, num_readonly)
    resolved_account_keys: Vec<Address>,
    instructions: Vec<solana_sdk::message::compiled_instruction::CompiledInstruction>,
    txn_meta: Option<UiTransactionStatusMeta>,
    rpc: &RpcClient,
) -> Result<Extraction> {
    //all accounts
    let (num_loaded_writable_address, num_loaded_readonly_address) =
        loaded_addresses.unwrap_or((0, 0));
    let num_signers = header.num_required_signatures as usize; // writetable unsigned accounts starts at index num_signers
    let readonly_signers_index = num_signers - header.num_readonly_signed_accounts as usize;
    let readonly_loaded_index = resolved_account_keys.len() - num_loaded_readonly_address as usize; //starting index of readonly loaded addresses
    let writable_loaded_index = readonly_loaded_index - num_loaded_writable_address as usize; //starting index of writable loaded addresses
    let readonly_unsigned_index =
        writable_loaded_index - header.num_readonly_unsigned_accounts as usize;

    let mut accounts: Vec<TxnAccount> = resolved_account_keys
        .iter()
        .enumerate()
        .map(|(i, acc)| {
            //first section in account_keys is writable and signers
            let mut signer = true;
            let mut readonly = false;
            // second section is readonly signers
            if i >= readonly_signers_index && i < num_signers {
                readonly = true;
            };
            // third section onwards are non-signers
            // third section is writable non-signers
            if i >= num_signers {
                signer = false;
            };
            // fourth section is readonly non-signers
            if i >= readonly_unsigned_index && i < writable_loaded_index {
                readonly = true;
            };
            // fifth section is writable loaded addresses
            // need no update, as signer is already reset and readonly is false by default

            // sixth section is the last and its readonly loaded addresses
            if i >= readonly_loaded_index {
                readonly = true;
            }

            (acc.clone(), signer, !readonly) //(address, is_signer, is_writable)
        })
        .collect();

    println!("Found {} txn accounts.", accounts.len());

    //transfers
    // [(program_id, from_wallet, optional(base_wallet), to_wallet, amount, optional(mint))]
    let mut transfers: Vec<Transfer> = vec![];
    for comp_insn in instructions {
        let program_id = comp_insn.program_id(resolved_account_keys.as_slice());
        // SOL transfers
        if program_id == &system_program_id {
            println!("debug - processing SOL transfer");
            //SystemInstruction::deserialize()
            let insn = bincode::deserialize::<SystemInstruction>(&comp_insn.data)?;
            println!("debug - deserialized system instruction.");
            match insn {
                SystemInstruction::Transfer { lamports } => {
                    // find the accounts
                    let from_idx = comp_insn.accounts[0];
                    let from = resolved_account_keys[from_idx as usize];
                    let to_idx = comp_insn.accounts[1];
                    let to = resolved_account_keys[to_idx as usize];

                    transfers.push((system_program_id, from, None, to, lamports, None));
                }
                SystemInstruction::TransferWithSeed {
                    lamports,
                    from_seed: _,
                    from_owner: _,
                } => {
                    // find the accounts
                    let from_idx = comp_insn.accounts[0];
                    let base_idx = comp_insn.accounts[0];
                    let to_idx = comp_insn.accounts[0];
                    let from = resolved_account_keys[from_idx as usize];
                    let base = resolved_account_keys[base_idx as usize];
                    let to = resolved_account_keys[to_idx as usize];

                    transfers.push((system_program_id, from, Some(base), to, lamports, None));
                }
                _ => continue,
            }
        };

        // spl-token transfers
        if program_id == &spl_token_interface::ID {
            println!("debug - processing spl transfer");
            let insn: TokenInstruction = TokenInstruction::unpack(&comp_insn.data)?;
            println!("debug - unpacked token instruction");
            match insn {
                TokenInstruction::Transfer { amount } => {
                    println!("transfer...");
                    let from_ata = resolved_account_keys[comp_insn.accounts[0] as usize];
                    let to_ata = resolved_account_keys[comp_insn.accounts[1] as usize];

                    let (from_ata_owner, to_ata_owner, mint_address) = resolve_transfer_addresses(
                        &txn_meta,
                        &resolved_account_keys,
                        &from_ata,
                        &to_ata,
                        rpc,
                        true,
                    )
                    .await?;
                    // to address or owner of the to_ata might not be present in the accounts, so add it
                    // even if it duplicates, it fine because we are only taking the first entry by conflict rule
                    accounts.push((to_ata_owner, false, false));

                    transfers.push((
                        spl_token_interface::ID,
                        from_ata_owner,
                        None,
                        to_ata_owner,
                        amount,
                        mint_address,
                    ));
                }
                TokenInstruction::TransferChecked {
                    amount,
                    decimals: _,
                } => {
                    println!("transfer-checked...");
                    let from_ata = resolved_account_keys[comp_insn.accounts[0] as usize];
                    let mint = resolved_account_keys[comp_insn.accounts[1] as usize];
                    let to_ata = resolved_account_keys[comp_insn.accounts[2] as usize];

                    let (from_ata_owner, to_ata_owner, _) = resolve_transfer_addresses(
                        &txn_meta,
                        &resolved_account_keys,
                        &from_ata,
                        &to_ata,
                        rpc,
                        false,
                    )
                    .await?;

                    // add destination ata onwer to the accounts
                    accounts.push((to_ata_owner, false, false));

                    transfers.push((
                        spl_token_interface::ID,
                        from_ata_owner,
                        None,
                        to_ata_owner,
                        amount,
                        Some(mint),
                    ));
                }
                _ => continue,
            };
        }

        // spl-token-2022 transfers
        if program_id == &spl_token_2022_interface::ID {
            println!("debug - processing token 2022 tranfers");
            let insn: std::result::Result<TokenInstruction2022, _> =
                TokenInstruction2022::unpack(comp_insn.data.as_slice());
            println!("debug - unpacked token 2022 instruction");
            match insn {
                #[allow(deprecated)]
                std::result::Result::Ok(TokenInstruction2022::Transfer { amount }) => {
                    println!("debug - token2022 transfer...");
                    let from_ata = resolved_account_keys[comp_insn.accounts[0] as usize];
                    let to_ata = resolved_account_keys[comp_insn.accounts[1] as usize];

                    let (from_ata_owner, to_ata_owner, mint_address) = resolve_transfer_addresses(
                        &txn_meta,
                        &resolved_account_keys,
                        &from_ata,
                        &to_ata,
                        rpc,
                        true,
                    )
                    .await?;

                    // add the destination ata owner to accounts
                    accounts.push((to_ata_owner, false, false));

                    transfers.push((
                        spl_token_2022_interface::ID,
                        from_ata_owner,
                        None,
                        to_ata_owner,
                        amount,
                        mint_address,
                    ));
                }
                std::result::Result::Ok(TokenInstruction2022::TransferChecked {
                    amount,
                    decimals: _,
                }) => {
                    println!("debug - token2022 transferchecked");
                    let from_ata = resolved_account_keys[comp_insn.accounts[0] as usize];
                    let mint = resolved_account_keys[comp_insn.accounts[1] as usize];
                    let to_ata = resolved_account_keys[comp_insn.accounts[2] as usize];

                    let (from_ata_owner, to_ata_owner, _) = resolve_transfer_addresses(
                        &txn_meta,
                        &resolved_account_keys,
                        &from_ata,
                        &to_ata,
                        rpc,
                        false,
                    )
                    .await?;

                    // add destination ata owner in the accounts
                    accounts.push((to_ata_owner, false, false));

                    transfers.push((
                        spl_token_2022_interface::ID,
                        from_ata_owner,
                        None,
                        to_ata_owner,
                        amount,
                        Some(mint),
                    ));
                }
                _ => continue,
            };
        }
    }

    Ok((accounts, transfers))
}

/// Resolve souce and destination ata owners, and mint address of the token transfer
pub async fn resolve_transfer_addresses(
    txn_meta: &Option<UiTransactionStatusMeta>,
    resolved_account_keys: &Vec<Address>,
    from_ata: &Address,
    to_ata: &Address,
    rpc: &RpcClient,
    mint_reqd: bool,
) -> Result<(Address, Address, Option<Address>)> {
    // first resolve the owner addresses from transaction meta
    if let Some(meta) = txn_meta {
        if let OptionSerializer::Some(ref post_token_balances) = meta.post_token_balances {
            let from_bal = post_token_balances
                .iter()
                .find(|bal| resolved_account_keys[bal.account_index as usize] == *from_ata);
            let to_bal = post_token_balances
                .iter()
                .find(|bal| resolved_account_keys[bal.account_index as usize] == *to_ata);

            if let (
                Some(UiTransactionTokenBalance {
                    account_index: _,
                    mint: bal_mint,
                    ui_token_amount: _,
                    owner: OptionSerializer::Some(from_ata_owner),
                    program_id: _,
                }),
                Some(UiTransactionTokenBalance {
                    account_index: _,
                    mint: _,
                    ui_token_amount: _,
                    owner: OptionSerializer::Some(to_ata_owner),
                    program_id: _,
                }),
            ) = (from_bal, to_bal)
            {
                return Ok((
                    Address::from_str_const(from_ata_owner),
                    Address::from_str_const(to_ata_owner),
                    if mint_reqd {
                        Some(Address::from_str_const(bal_mint))
                    } else {
                        None
                    },
                ));
            }
        };
    };

    // when the transaction meta is not available fetch the owner from the rpc
    let (from_ata_owner, mint_f) = match rpc.get_account(&from_ata).await {
        std::result::Result::Ok(from_acc) => {
            if from_acc.owner == spl_token_interface::ID {
                Account::unpack(&from_acc.data)
                    .map(|d| (d.owner, Some(d.mint)))
                    .unwrap_or((*from_ata, None))
            } else {
                Account2022::unpack(&from_acc.data)
                    .map(|d| (d.owner, Some(d.mint)))
                    .unwrap_or((*from_ata, None))
            }
        }
        std::result::Result::Err(e) => {
            eprintln!(
                "Failed to fetch from ata owner with error\n{:?}\n\nUsing ata itselves",
                e
            );
            (*from_ata, None)
        }
    };

    let (to_ata_owner, mint_t) = match rpc.get_account(&to_ata).await {
        std::result::Result::Ok(to_acc) => {
            if to_acc.owner == spl_token_interface::ID {
                Account::unpack(&to_acc.data)
                    .map(|d| (d.owner, Some(d.mint)))
                    .unwrap_or((*to_ata, None))
            } else {
                Account2022::unpack(&to_acc.data)
                    .map(|d| (d.owner, Some(d.mint)))
                    .unwrap_or((*to_ata, None))
            }
        }
        std::result::Result::Err(e) => {
            eprintln!(
                "Failed to fetch to ata owner with error\n{:?}\n\nUsing ata itselves",
                e
            );
            (*to_ata, None)
        }
    };

    Ok((
        from_ata_owner,
        to_ata_owner,
        if mint_f.is_some() { mint_f } else { mint_t },
    ))
}

pub async fn batch_insert_into_accounts(
    accounts: &Vec<TxnAccount>,
    slot: i64,
    executor: &mut PgConnection,
) -> Result<PgQueryResult> {
    // the trailing space in the initial sql fragment is important to not get errors, and as query builder doesn't automatically appends it
    let mut accounts_qb: QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("INSERT INTO accounts (pubkey, first_seen_slot) ");
    accounts_qb.push_values(accounts.iter().map(|account| account.0), |mut b, pubkey| {
        b.push_bind(pubkey.to_string()).push_bind(slot);
    });
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
        |mut b, (program_id, from_address, base_address, to_address, amount, mint_address)| {
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
