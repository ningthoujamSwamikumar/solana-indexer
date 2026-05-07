use anyhow::{Ok, Result};
use base64::Engine;
use solana_client::{
    rpc_client::RpcClient,
    rpc_request::Address,
    rpc_response::{
        EncodedTransaction,
        transaction::{VersionedMessage, versioned::VersionedTransaction},
    },
};

use solana_sdk::program_pack::Pack;
use solana_system_interface::{instruction::SystemInstruction, program::ID as system_program_id};

use spl_token_2022_interface::{
    instruction::TokenInstruction as TokenInstruction2022, state::Account as Account2022,
};
use spl_token_interface::{instruction::TokenInstruction, state::Account};

//use solana_program::example_mocks::solana_sdk::{system_instruction, system_program};
use sqlx::{Pool, Postgres};

/// Find all the transactions present in the transactions table and fill their corresponding entries into new tables created <br/>
/// like accounts, and transfers
async fn backfill_transfers_accounts(pg_pool: Pool<Postgres>, rpc: RpcClient) -> Result<()> {
    //go through all recorded trannsactions
    let txns: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT signature, slot, tx_base64 FROM trannsactions;")
            .fetch_all(&pg_pool)
            .await?;
    for (sig, slot, tx_bs64) in txns {
        //extract programs, accounts, datas
        let tx_bytes = base64::engine::general_purpose::STANDARD.decode(&tx_bs64)?;
        let txn: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
        let (accounts, transfers) = match txn.message {
            VersionedMessage::Legacy(msg) => {
                //accounts
                let num_signers = msg.header.num_required_signatures as usize; // writetable unsigned accounts starts at index num_signers
                let readonly_signers_index =
                    num_signers - msg.header.num_readonly_signed_accounts as usize;
                let readonly_unsigned_index =
                    msg.account_keys.len() - msg.header.num_readonly_unsigned_accounts as usize;
                let accounts: Vec<(Address, bool, bool)> = msg
                    .account_keys
                    .iter()
                    .enumerate()
                    .map(|(i, acc)| {
                        let mut signer = true;
                        let mut readonly = false;
                        if i >= num_signers as usize {
                            signer = false;
                        };
                        if i >= readonly_signers_index && i <= num_signers {
                            readonly = true;
                        };
                        if i >= readonly_unsigned_index {
                            readonly = true;
                        };

                        (acc.clone(), signer, !readonly) //(address, is_signer, is_writable)
                    })
                    .collect();

                //transfers
                // [(program_id, from_wallet, optional(base_wallet), to_wallet, amount, optional(mint))]
                let mut transfers: Vec<(
                    Address,
                    Address,
                    Option<Address>,
                    Address,
                    u64,
                    Option<Address>,
                )> = vec![];
                for comp_insn in msg.instructions {
                    let program_id = comp_insn.program_id(msg.account_keys.as_slice());
                    // SOL transfers
                    if program_id == &system_program_id {
                        let insn = bincode::deserialize::<SystemInstruction>(&comp_insn.data)?;
                        match insn {
                            SystemInstruction::Transfer { lamports } => {
                                // find the accounts
                                let from_idx = comp_insn.accounts[0];
                                let from = msg.account_keys[from_idx as usize];
                                let to_idx = comp_insn.accounts[1];
                                let to = msg.account_keys[to_idx as usize];

                                transfers.push((system_program_id, from, None, to, lamports, None));
                            }
                            SystemInstruction::TransferWithSeed {
                                lamports,
                                from_seed,
                                from_owner,
                            } => {
                                // find the accounts
                                let from_idx = comp_insn.accounts[0];
                                let base_idx = comp_insn.accounts[0];
                                let to_idx = comp_insn.accounts[0];
                                let from = msg.account_keys[from_idx as usize];
                                let base = msg.account_keys[base_idx as usize];
                                let to = msg.account_keys[to_idx as usize];

                                transfers.push((
                                    system_program_id,
                                    from,
                                    Some(base),
                                    to,
                                    lamports,
                                    None,
                                ));
                            }
                            _ => continue,
                        }
                    };

                    // spl-token transfers
                    if program_id == &spl_token_interface::ID {
                        let insn: TokenInstruction = TokenInstruction::unpack(&comp_insn.data)?;
                        match insn {
                            TokenInstruction::Transfer { amount } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[1] as usize];

                                let from_ata_data_bytes = rpc.get_account_data(&from_ata)?;
                                let from_ata_data = Account::unpack(&from_ata_data_bytes)?;

                                let to_ata_data_bytes = rpc.get_account_data(&to_ata)?;
                                let to_ata_data = Account::unpack(&to_ata_data_bytes)?;

                                transfers.push((
                                    spl_token_interface::ID,
                                    from_ata_data.owner,
                                    None,
                                    to_ata_data.owner,
                                    amount,
                                    Some(from_ata_data.mint),
                                ));
                            }
                            TokenInstruction::TransferChecked { amount, decimals } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let mint = msg.account_keys[comp_insn.accounts[1] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[2] as usize];

                                let from_data = rpc.get_account_data(&from_ata)?;
                                let from = Account::unpack(&from_data)?;

                                let to_data = rpc.get_account_data(&to_ata)?;
                                let to = Account::unpack(&to_data)?;

                                transfers.push((
                                    spl_token_interface::ID,
                                    from.owner,
                                    None,
                                    to.owner,
                                    amount,
                                    Some(mint),
                                ));
                            }
                            _ => continue,
                        };
                    }

                    // spl-token-2022 transfers
                    if program_id == &spl_token_2022_interface::ID {
                        let insn: TokenInstruction2022 =
                            TokenInstruction2022::unpack(&comp_insn.data)?;
                        match insn {
                            TokenInstruction2022::Transfer { amount } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[1] as usize];

                                let from_ata_data_bytes = rpc.get_account_data(&from_ata)?;
                                let from_ata_data: Account2022 =
                                    Account2022::unpack(&from_ata_data_bytes)?;

                                let to_ata_data_bytes = rpc.get_account_data(&to_ata)?;
                                let to_ata_data: Account2022 =
                                    Account2022::unpack(&to_ata_data_bytes)?;

                                transfers.push((
                                    spl_token_2022_interface::ID,
                                    from_ata_data.owner,
                                    None,
                                    to_ata_data.owner,
                                    amount,
                                    Some(from_ata_data.mint),
                                ));
                            }
                            TokenInstruction2022::TransferChecked { amount, decimals } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let mint = msg.account_keys[comp_insn.accounts[1] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[2] as usize];

                                let from_data = rpc.get_account_data(&from_ata)?;
                                let from: Account2022 = Account2022::unpack(&from_data)?;

                                let to_data = rpc.get_account_data(&to_ata)?;
                                let to: Account2022 = Account2022::unpack(&to_data)?;

                                transfers.push((
                                    spl_token_2022_interface::ID,
                                    from.owner,
                                    None,
                                    to.owner,
                                    amount,
                                    Some(mint),
                                ));
                            }
                            _ => continue,
                        };
                    }
                }

                (accounts, transfers)
            }
            VersionedMessage::V0(msg) => {
                //accounts
                let num_signers = msg.header.num_required_signatures as usize; // writetable unsigned accounts starts at index num_signers
                let readonly_signers_index =
                    num_signers - msg.header.num_readonly_signed_accounts as usize;
                let readonly_unsigned_index =
                    msg.account_keys.len() - msg.header.num_readonly_unsigned_accounts as usize;
                let accounts: Vec<(Address, bool, bool)> = msg
                    .account_keys
                    .iter()
                    .enumerate()
                    .map(|(i, acc)| {
                        let mut signer = true;
                        let mut readonly = false;
                        if i >= num_signers as usize {
                            signer = false;
                        };
                        if i >= readonly_signers_index && i <= num_signers {
                            readonly = true;
                        };
                        if i >= readonly_unsigned_index {
                            readonly = true;
                        };

                        (acc.clone(), signer, !readonly) //(address, is_signer, is_writable)
                    })
                    .collect();

                //transfers
                // [(program_id, from_wallet, optional(base_wallet), to_wallet, amount, optional(mint))]
                let mut transfers: Vec<(
                    Address,
                    Address,
                    Option<Address>,
                    Address,
                    u64,
                    Option<Address>,
                )> = vec![];
                for comp_insn in msg.instructions {
                    let program_id = comp_insn.program_id(msg.account_keys.as_slice());
                    // SOL transfers
                    if program_id == &system_program_id {
                        let insn = bincode::deserialize::<SystemInstruction>(&comp_insn.data)?;
                        match insn {
                            SystemInstruction::Transfer { lamports } => {
                                // find the accounts
                                let from_idx = comp_insn.accounts[0];
                                let from = msg.account_keys[from_idx as usize];
                                let to_idx = comp_insn.accounts[1];
                                let to = msg.account_keys[to_idx as usize];

                                transfers.push((system_program_id, from, None, to, lamports, None));
                            }
                            SystemInstruction::TransferWithSeed {
                                lamports,
                                from_seed,
                                from_owner,
                            } => {
                                // find the accounts
                                let from_idx = comp_insn.accounts[0];
                                let base_idx = comp_insn.accounts[0];
                                let to_idx = comp_insn.accounts[0];
                                let from = msg.account_keys[from_idx as usize];
                                let base = msg.account_keys[base_idx as usize];
                                let to = msg.account_keys[to_idx as usize];

                                transfers.push((
                                    system_program_id,
                                    from,
                                    Some(base),
                                    to,
                                    lamports,
                                    None,
                                ));
                            }
                            _ => continue,
                        }
                    };

                    // spl-token transfers
                    if program_id == &spl_token_interface::ID {
                        let insn: TokenInstruction = TokenInstruction::unpack(&comp_insn.data)?;
                        match insn {
                            TokenInstruction::Transfer { amount } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[1] as usize];

                                let from_ata_data_bytes = rpc.get_account_data(&from_ata)?;
                                let from_ata_data = Account::unpack(&from_ata_data_bytes)?;

                                let to_ata_data_bytes = rpc.get_account_data(&to_ata)?;
                                let to_ata_data = Account::unpack(&to_ata_data_bytes)?;

                                transfers.push((
                                    spl_token_interface::ID,
                                    from_ata_data.owner,
                                    None,
                                    to_ata_data.owner,
                                    amount,
                                    Some(from_ata_data.mint),
                                ));
                            }
                            TokenInstruction::TransferChecked { amount, decimals } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let mint = msg.account_keys[comp_insn.accounts[1] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[2] as usize];

                                let from_data = rpc.get_account_data(&from_ata)?;
                                let from = Account::unpack(&from_data)?;

                                let to_data = rpc.get_account_data(&to_ata)?;
                                let to = Account::unpack(&to_data)?;

                                transfers.push((
                                    spl_token_interface::ID,
                                    from.owner,
                                    None,
                                    to.owner,
                                    amount,
                                    Some(mint),
                                ));
                            }
                            _ => continue,
                        };
                    }

                    // spl-token-2022 transfers
                    if program_id == &spl_token_2022_interface::ID {
                        let insn: TokenInstruction2022 =
                            TokenInstruction2022::unpack(&comp_insn.data)?;
                        match insn {
                            TokenInstruction2022::Transfer { amount } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[1] as usize];

                                let from_ata_data_bytes = rpc.get_account_data(&from_ata)?;
                                let from_ata_data: Account2022 =
                                    Account2022::unpack(&from_ata_data_bytes)?;

                                let to_ata_data_bytes = rpc.get_account_data(&to_ata)?;
                                let to_ata_data: Account2022 =
                                    Account2022::unpack(&to_ata_data_bytes)?;

                                transfers.push((
                                    spl_token_2022_interface::ID,
                                    from_ata_data.owner,
                                    None,
                                    to_ata_data.owner,
                                    amount,
                                    Some(from_ata_data.mint),
                                ));
                            }
                            TokenInstruction2022::TransferChecked { amount, decimals } => {
                                let from_ata = msg.account_keys[comp_insn.accounts[0] as usize];
                                let mint = msg.account_keys[comp_insn.accounts[1] as usize];
                                let to_ata = msg.account_keys[comp_insn.accounts[2] as usize];

                                let from_data = rpc.get_account_data(&from_ata)?;
                                let from: Account2022 = Account2022::unpack(&from_data)?;

                                let to_data = rpc.get_account_data(&to_ata)?;
                                let to: Account2022 = Account2022::unpack(&to_data)?;

                                transfers.push((
                                    spl_token_2022_interface::ID,
                                    from.owner,
                                    None,
                                    to.owner,
                                    amount,
                                    Some(mint),
                                ));
                            }
                            _ => continue,
                        };
                    }
                }

                (accounts, transfers)
            }
        };

        //insert accounts into accounts
        //insert transfers into transfers
        //insert transaction accounts into transaction_accounts
    }

    Ok(())
}
