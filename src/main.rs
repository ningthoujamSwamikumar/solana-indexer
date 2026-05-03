use anyhow::{Result, anyhow};
use serde_json::json;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{CommitmentConfig, RpcBlockConfig},
    rpc_request::RpcRequest,
};

const MAINNET_URL: &str = "https://api.mainnet.solana.com";
const DEVNET_URL: &str = "https://api.devnet.solana.com";
const LOCALNET_URL: &str = "http://localhost:8899";

#[tokio::main]
async fn main() -> Result<()> {
    println!("Hello, world!");

    let rpc_client =
        RpcClient::new_with_commitment(MAINNET_URL.to_string(), CommitmentConfig::finalized());
    let client_result: u64 = rpc_client
        .send(RpcRequest::GetSlot, json!([{"commitment": "finalized"}]))
        .await?;
    println!("client result: {client_result}");

    let slot = rpc_client.get_slot().await?;
    println!("slot: {slot}");

    //assert_eq!(client_result, slot);

    let block = rpc_client
        .get_block_with_config(
            slot,
            RpcBlockConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
                ..Default::default()
            },
        )
        .await?;
    // let transactions = match block.transactions {
    //     Some(txns) => txns,
    //     None => vec![],
    // };

    let transactions = block.transactions.unwrap_or(vec![]);
    //println!("transactions in the block: {transactions:?}");
    println!("#transactions in the block: {}", transactions.len());

    Ok(())
}
