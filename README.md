# Real-time stream of block events library

A library for consuming and subscribing to new block events in real-time from different sources:
 - [ ] mempool.space - [WebSocket](https://mempool.space/docs/api/websocket) and [REST](https://mempool.space/docs/api/rest) APIs (under development)
 - [ ] bitcoin core RPC `#TODO`
 - [ ] bitcoin P2P `#TODO`

It's useful for projects to get notified for connected and disconnected new blocks, currently using the following as output in async manner:
``` rust
pub enum BlockEvent {
    Connected(BlockExtended),
    Disconnected((u32, BlockHash)),
    Error(),
}
```

Can be also used through command-line interface (CLI), as `block-explorer-cli` and just passing the network: Regtest, Signet, Testnet and Mainnet.

> **NOTE**: The previous implemented track-address feature and other data, such as: mempool-blocks, stats... has been deprecated and it's not refactored yet.
## Requirements:
To use the library as CLI or to contribute you must have rust and cargo installed in your computer, you can check it with:

``` sh
# check rust version, it should return its version if installed
rustc --version
# check cargo version, it should return its version if installed
cargo --version
```
If you do not have it installed, you can follow this tutorial from [The Rust Programming Language book](https://doc.rust-lang.org/book/ch01-01-installation.html)

## Compiling and using the CLI:
To compile and use it as a command in terminal with no need of cargo, you can use the following command:
``` sh
# from inside this repo
cargo install --path .
```
## Examples:
### Consuming new block events through the CLI:
``` sh
# testnet connection is set by default
cargo run -- data-stream --blocks

# to use regtest, you need to pass it as a parameter
cargo run -- --base-url localhost:8999/testnet/api/v1 data-stream --blocks
```
### Subscribing and consuming new block events through the lib:
``` rust
use anyhow::{self, Ok};
use futures_util::{pin_mut, StreamExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // for mempool.space regtest network
    let base_url = "localhost:8999/testnet/api/v1";

    // no checkpoint for this example, check the other one if interested.
    let checkpoint = None;

    // async fetch the block-events stream through the lib
    let block_events = block_events::subscribe_to_block_headers(base_url, checkpoint).await?;

    // consume and execute your code (current only matching and printing) in async manner for each new block-event
    pin_mut!(block_events);
    while let Some(block_event) = block_events.next().await {
        match block_event {
            block_events::api::BlockEvent::Connected(block) => {
                println!(
                    "[connected block][block_hash {:#?}][block_prev_hash {:#?}]",
                    block.block_hash(),
                    block.prev_blockhash
                );
            }
            block_events::api::BlockEvent::Disconnected((height, block_hash)) => {
                println!(
                    "[disconnected block][height {:#?}][block_hash: {:#?}]",
                    height, block_hash
                );
            }
        }
    }
    Ok(())
}
```