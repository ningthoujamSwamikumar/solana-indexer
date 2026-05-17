pub mod accounts_and_transfers;
pub mod batch_inserts;
pub mod blocks;

use solana_client::rpc_request::Address;

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
