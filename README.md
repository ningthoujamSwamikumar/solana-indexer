# solana-indexer
The aim of this project is to build a reliable and performant at scale indexer, with reorg (chain reorganization) handling and structured decoding of programs which can be used as a query layer for real use case.

## Features:
- High throughput Solana indexer with real-time and historical sync
- Reorg-safe pipeline with rollback and idempotent processing
- Structured decoding of SOL and idempotent processing
- Optimized relational schema for wallet for wallet and token queries
- Concurrent ingestion using async Rust
- Hybrid architecture: RPC backfill + Websocket streaming
- Fault-tolerant with automatic resume from last processed slot
- REST API for querying indexed blockchain data
- Powers wallet analytics and transaction insights
- Dockerized for easy deployments

# Roadmap
- Phase 0: Cli that prints latest block data ✅
    > Goal: Learn the basics
    > - Fetch latest slot
    > - Fetch a block
    > - print transactions
- Phase 1: End to end pipeline
    > Goal: Index blocks continuously
    > - fetch slot -> fetch block -> store in DB -> loop
    > - Simple DB schema: slots and transactions (raw JSON)
- Phase 2: Add structure to the data to be usable
    > Goal: Get all transfers for a wallet
    > - decode SOL transfers and SPL token transfers
    > - normalize tables - transfers and accounts
    > - use Anchor framework IDL (if needed) 
- Phase 3: Make it Reliable
    > Goal: Incremental Sync + Backfill
    > - Start from a given slot
    > - Resume after restart
    > - Backfill past data
    > - store last processed slot
    > - Kill process -> restart -> continues correctly
- Phase 4: Move from batch to live system
    > Goal: Streaming + Near Real-time
    > - Websocket subscription (logs/slots)
    > - Hybrid model - backfill (RPC) and live updates (WS)
    > - Deliverable: live updates within seconds
- Phase 5: Data integrity & Regorg safety
    > Goal: System survives inconsistencies
    > - store parent slot
    > - detect inconsistencies
    > - use `finalized` OR rollback mechanism
- Phase 6: Performance and Scaling
    > Goal: Can process high throughput without crashing
    > - batching inserts
    > - concurrency (Tokio)
    > - queue (Kafka)
- Phase 7: Api Layer
    > Goal: Usable Backend service
    > - Rest api (Rust)
    > - wallet transfers endpoint
    > - token activity endpoint
    > - recent transactions endpoint
- Phase 8: Product
    > Goal: Turn infra to product
    > - One of: Wallet explorer or Token Analytics dashboard or NFT activity tracker 
- Phase 9: Advance Decoding
    > Goal: Stand out by showing domain specific insights
    > - decode specific programs- DEX trades and NFT mints
    > - keep on adding programs time to time
- Phase 10: Production Polish
    > Goal: Repo that looks like a real system, not a demo
    > - logging + metrics
    > - retry strategies
    > - config-driven pipelines
    > - Dockerized deployment
    > - documentation