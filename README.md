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
use futures::{pin_mut, StreamExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // for mempool.space testnet network
    let http_base_url = "http://mempool.space/testnet/api/";
    let ws_base_url = "wss://mempool.space/testnet";

    // no checkpoint for this example, but you could use the following one to test it by yourself (in mainnet).
    // checkpoint for first BDK Taproot transaction on mainnet (base_url update needed)
    // let checkpoint = (709635, bitcoin::BlockHash::from("00000000000000000001f9ee4f69cbc75ce61db5178175c2ad021fe1df5bad8f"));
    let checkpoint = None;

    // async fetch the block-events stream through the lib
    let block_events =
        block_events::subscribe_to_block_headers(http_base_url, ws_base_url, checkpoint).await?;

    // consume and execute your code (current only matching and printing) in async manner for each new block-event
    pin_mut!(block_events);
    while let Some(block_event) = block_events.next().await {
        match block_event? {
            block_events::api::BlockEvent::Connected(block_header) => {
                println!("[connected][block_header] {:#?}", block_header);
            }
            block_events::api::BlockEvent::Disconnected((height, block_hash)) => {
                println!(
                    "[disconnected][height: {:#?}][block_hash: {:#?}]",
                    height, block_hash
                );
            }
        }
    }
    Ok(())
}

```