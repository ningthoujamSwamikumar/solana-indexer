use serde::Serialize;
use sqlx::prelude::FromRow;

#[derive(FromRow, Serialize)]
pub struct Block {
    pub slot: i64,
    pub blockhash: String,
    pub block_time: i64,
    pub parent_slot: i64,
}
