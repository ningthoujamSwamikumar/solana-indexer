use axum::{Json, Router, extract::State, routing};
use sqlx::{PgPool, Pool, Postgres};
use tokio::net::TcpListener;

use crate::{daos::Block, errors::ApiError};

mod daos;
mod errors;

#[tokio::main]
async fn main() -> () {
    println!("Hello World from Apis");

    let pg_pool = PgPool::connect("postgresql://postgres@localhost:5432/solana_index")
        .await
        .expect("Database connection failed!");

    let app_router = Router::new()
        .route("/", routing::get(hello_world))
        .route("/blocks", routing::get(get_blocks))
        .with_state(pg_pool);

    let listener = TcpListener::bind("127.0.0.1:2345")
        .await
        .expect("Failed to bind the given address!");

    axum::serve(listener, app_router)
        .await
        .expect("Server failed to start!");
}

async fn hello_world() -> &'static str {
    "hello_world"
}

async fn get_blocks(State(pg_pool): State<Pool<Postgres>>) -> Result<Json<Vec<Block>>, ApiError> {
    let blocks = sqlx::query_as::<_, Block>("SELECT * from blocks;")
        .fetch_all(&pg_pool)
        .await
        .map_err(|e| ApiError::InternalServerError(Some(e.to_string())))?;

    Ok(Json(blocks))
}

