use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing,
};
use serde::Deserialize;
use solana_indexer::daos::{BlockDao, TransferDao};
use sqlx::{PgPool, Pool, Postgres, QueryBuilder};
use tokio::net::TcpListener;

use crate::errors::ApiError;

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
        .route("/transfers", routing::get(get_transfers))
        .route("/transfers/{txn_sig}", routing::get(get_transfers_by_txn))
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

async fn get_blocks(
    State(pg_pool): State<Pool<Postgres>>,
) -> Result<Json<Vec<BlockDao>>, ApiError> {
    let blocks = sqlx::query_as::<_, BlockDao>("SELECT * from blocks;")
        .fetch_all(&pg_pool)
        .await
        .map_err(|e| ApiError::InternalServerError(Some(e.to_string())))?;

    Ok(Json(blocks))
}

#[derive(Deserialize)]
struct GetTransfersQuery {
    last_id: Option<String>,
    page_size: Option<i64>,
    address: Option<String>,
}

async fn get_transfers(
    state: State<Pool<Postgres>>,
    Query(GetTransfersQuery {
        last_id,
        page_size,
        address,
    }): Query<GetTransfersQuery>,
) -> Result<Json<Vec<TransferDao>>, ApiError> {
    get_transfers_inner(state, Path(None), last_id, page_size, address).await
}

async fn get_transfers_by_txn(
    state: State<Pool<Postgres>>,
    Path(txn_sig): Path<String>,
    Query(GetTransfersQuery {
        last_id,
        page_size,
        address,
    }): Query<GetTransfersQuery>,
) -> Result<Json<Vec<TransferDao>>, ApiError> {
    get_transfers_inner(
        state,
        Path::from(axum::extract::Path(Some(txn_sig))),
        last_id,
        page_size,
        address,
    )
    .await
}

async fn get_transfers_inner(
    State(pg_pool): State<Pool<Postgres>>,
    Path(txn_sig): Path<Option<String>>,
    last_id: Option<String>,
    page_size: Option<i64>,
    address: Option<String>,
) -> Result<Json<Vec<TransferDao>>, ApiError> {
    let last = last_id.unwrap_or("".to_string());
    let page = page_size.unwrap_or(50);

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM transfers WHERE ");

    if let Some(sig) = txn_sig {
        qb.push(" txn_signature = ").push_bind(sig);
    } else {
        if let Some(ref addr) = address {
            qb.push(" from_address = ")
                .push_bind(addr)
                .push(" OR to_address = ")
                .push_bind(addr)
                .push(" OR base_address = ")
                .push_bind(addr);
        }

        qb.push(" txn_signature > ")
            .push_bind(last)
            .push(" ORDER BY txn_signature LIMIT ")
            .push_bind(page);
    };

    let transfers: Vec<TransferDao> = qb
        .build_query_as()
        .fetch_all(&pg_pool)
        .await
        .map_err(|e| ApiError::InternalServerError(Some(e.to_string())))?;

    print!("Received a request");

    Ok(Json(transfers))
}
