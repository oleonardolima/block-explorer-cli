// Block Events Library
// Written in 2022 by Leonardo Lima <> and Lloyd Fournier <>
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! # Block Events Library
//!
//! This a simple, concise and lightweight library for subscribing to real-time stream of blocks from multiple sources.
//!
//! It focuses on providing a simple API and [`BlockEvent`] response type for clients to consume
//! any new events or starting from a pre-determined checkpoint.
//!
//! The library produces [`BlockEvent::Connected`] and [`BlockEvent::Disconnected`] events by handling reorganization
//! events and blockchain forks.
//!
//! The library works in an `async` fashion producing a Rust stream of [`BlockEvent`].
//!
//! It is a project under development during the Summer of Bitcoin'22 @BitcoinDevKit, if you would like to know more
//! please check out the repository, project proposal or reach out.
//!
//! # Examples
//! ## Subscribe to all new block events for mempool.space
//! ``` no_run
//! use anyhow::{self, Ok};
//! use futures_util::{pin_mut, StreamExt};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     env_logger::init();
//!
//!     // for mempool.space regtest network
//!     let base_url = "localhost:8999/testnet/api/v1";
//!
//!     // no checkpoint for this example, check the commented other one if interested.
//!     // checkpoint for first BDK Taproot transaction on mainnet (base_url update needed)
//!     // let checkpoint = (709635, bitcoin::BlockHash::from("00000000000000000001f9ee4f69cbc75ce61db5178175c2ad021fe1df5bad8f"));
//!     let checkpoint = None;
//!
//!     // async fetch the block-events stream through the lib
//!     let block_events = block_events::subscribe_to_block_headers(base_url, checkpoint).await?;
//!
//!     // consume and execute your code (current only matching and printing) in async manner for each new block-event
//!     pin_mut!(block_events);
//!     while let Some(block_event) = block_events.next().await {
//!         match block_event {
//!             block_events::api::BlockEvent::Connected(block) => {
//!                 println!(
//!                     "[connected block][block_hash {:#?}][block_prev_hash {:#?}]",
//!                     block.block_hash(),
//!                     block.prev_blockhash
//!                 );
//!             }
//!             block_events::api::BlockEvent::Disconnected((height, block_hash)) => {
//!                 println!(
//!                     "[disconnected block][height {:#?}][block_hash: {:#?}]",
//!                     height, block_hash
//!                 );
//!             }
//!         }
//!     }
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod http;
pub mod websocket;

pub extern crate async_stream;
pub extern crate bitcoin;
pub extern crate tokio;
pub extern crate tokio_tungstenite;

use std::{collections::HashMap, collections::VecDeque, pin::Pin};

use api::{BlockEvent, BlockExtended};
use futures::{Stream, StreamExt, TryStream};
use http::HttpClient;

use anyhow::{anyhow, Ok};
use async_stream::try_stream;
use bitcoin::{
    blockdata::constants::genesis_block, Block, BlockHash, BlockHeader, Network, Transaction,
};

const DEFAULT_CONCURRENT_REQUESTS: u8 = 4;

/// A simple cache struct to store the all fetched and new blocks in-memory
///
/// It's used in order to handle reorganization events, and produce both connected and disconnected events
#[derive(Debug, Clone)]
pub struct BlockHeadersCache {
    pub tip: BlockHash,
    pub active_headers: HashMap<BlockHash, BlockHeader>,
    pub stale_headers: HashMap<BlockHash, BlockHeader>,
}

impl BlockHeadersCache {
    /// Create a new instance of [`BlockHeadersCache`] for given checkpoint (height: [`u32`], hash: [`BlockHash`])
    ///
    /// It creates with the checkpoint block as tip and active_headers
    pub async fn new(base_url: &str) -> anyhow::Result<BlockHeadersCache> {
        let http_client = HttpClient::new(base_url);

        let hash = http_client.get_tip_hash().await?;
        let header = http_client.get_block_header(hash).await?;

        Ok(BlockHeadersCache {
            tip: hash,
            active_headers: HashMap::from([(hash, header)]),
            stale_headers: HashMap::new(),
        })
    }

    /// Create a new instance of [`BlockHeadersCache`] for given [`Network`]
    ///
    /// It creates with the genesis block for given network as tip and active_headers
    pub fn new_with_genesis(network: Network) -> BlockHeadersCache {
        let genesis_block = genesis_block(network);

        BlockHeadersCache {
            tip: genesis_block.block_hash(),
            active_headers: HashMap::from([(genesis_block.block_hash(), genesis_block.header)]),
            stale_headers: HashMap::new(),
        }
    }

    /// Create a new instance of [`BlockHeadersCache`] for given checkpoint (height: [`u32`], hash: [`BlockHash`])
    ///
    /// It creates with the checkpoint block as tip and active_headers
    pub async fn new_with_checkpoint(
        base_url: &str,
        checkpoint: (u32, BlockHash),
    ) -> anyhow::Result<BlockHeadersCache> {
        let (_, hash) = checkpoint;

        let header = HttpClient::new(base_url).get_block_header(hash).await?;

        Ok(BlockHeadersCache {
            tip: hash,
            active_headers: HashMap::from([(hash, header)]),
            stale_headers: HashMap::new(),
        })
    }

    /// Validate if the new [`BlockHeader`] or [`BlockExtended`] candidate is a valid tip
    ///
    /// Updates the [`BlockHeadersCache`] state, updating the tip, extending the active_headers and returns a boolean
    pub fn validate_new_header(&mut self, candidate: BlockHeader) -> bool {
        // TODO: (@leonardo.lima) It should check and validate the PoW for the header candidates
        if self.tip == candidate.prev_blockhash {
            self.tip = candidate.block_hash();
            self.active_headers
                .insert(candidate.block_hash(), candidate);
            return true;
        }
        false
    }

    /// Find common ancestor for current active chain and the fork chain candidate
    ///
    /// Updates the [`BlockHeadersCache`] state with fork chain candidates
    ///
    /// Returns a common ancestor [`BlockHeader`] stored in [`BlockHeadersCache`] and the
    /// fork branch chain as a [`VecDeque<BlockHeader>`]
    pub async fn find_or_fetch_common_ancestor(
        &self,
        http_client: HttpClient,
        fork_candidate: BlockHeader,
    ) -> anyhow::Result<(BlockHeader, VecDeque<BlockHeader>)> {
        let mut common_ancestor = fork_candidate;
        let mut fork_branch: VecDeque<BlockHeader> = VecDeque::new();
        while !self
            .active_headers
            .contains_key(&common_ancestor.block_hash())
        {
            fork_branch.push_back(common_ancestor);
            common_ancestor = http_client
                .get_block_header(common_ancestor.prev_blockhash)
                .await?;
        }
        Ok((common_ancestor, fork_branch))
    }

    /// Rollback active chain in [`BlockHeadersCache`] back to passed [`BlockHeader`]
    ///
    /// Returns all stale, and to be disconnected blocks as a [`VecDeque<BlockHeader>`]
    pub async fn rollback_active_chain(
        &mut self,
        header: BlockHeader,
    ) -> anyhow::Result<VecDeque<BlockHeader>> {
        let mut disconnected = VecDeque::new();
        while header.block_hash() != self.tip {
            let (stale_hash, stale_header) = self.active_headers.remove_entry(&self.tip).unwrap();
            disconnected.push_back(stale_header);

            self.stale_headers.insert(stale_hash, stale_header);
            self.tip = stale_header.prev_blockhash;
        }
        Ok(disconnected)
    }

    /// Apply fork branch to active chain, and update tip to new [`BlockHeader`]
    ///
    /// Returns the new tip [`BlockHash`], and the connected block headers as a [`VecDeque<BlockHeader>`]
    pub fn apply_fork_chain(
        &mut self,
        mut fork_branch: VecDeque<BlockHeader>,
    ) -> anyhow::Result<(BlockHash, VecDeque<BlockHeader>)> {
        let mut connected = VecDeque::new();
        while !fork_branch.is_empty() {
            let header = fork_branch.pop_front().unwrap();
            connected.push_back(header);

            self.active_headers.insert(header.block_hash(), header);
            self.tip = header.block_hash();
        }
        Ok((self.tip, connected))
    }
}

/// Process all candidates listened from source, it tries to apply the candidate to current active chain cached
/// It handles reorganization and fork if needed
/// Steps:
///  - validates if current candidate is valid as a new tip, if valid extends chain producing [`BlockEvent::Connected`]
///  - otherwise, find common ancestor between branches
///  - rollback current cached active chain
///  - apply forked branch, and produces [`BlockEvent::Disconnected`] for staled blocks and [`BlockEvent::Connected`]
///    for new branch
async fn process_candidates(
    base_url: &str,
    mut cache: BlockHeadersCache,
    mut candidates: Pin<Box<dyn Stream<Item = anyhow::Result<BlockHeader>>>>,
) -> anyhow::Result<impl TryStream<Item = anyhow::Result<BlockEvent<BlockHeader>>>> {
    let http_client = HttpClient::new(base_url);

    let stream = try_stream! {
        // TODO: (@leonardo.lima) Do not just propagate the errors, add a retry mechanism instead
        while let candidate = candidates.next().await.ok_or(anyhow!("the `bitcoin::BlockHeader` candidate is None"))?? {
            // validate if the [`BlockHeader`] candidate is a valid new tip
            // yields a [`BlockEvent::Connected()`] variant and continue the iteration
            if cache.validate_new_header(candidate) {
                yield BlockEvent::Connected(BlockHeader::from(candidate.clone()));
                continue
            }

            // find common ancestor for current active chain and the forked chain
            // fetches forked chain candidates and store in cache
            let (common_ancestor, fork_chain) = cache.find_or_fetch_common_ancestor(http_client.clone(), candidate).await?;

            // rollback current active chain, moving blocks to staled field
            // yields BlockEvent::Disconnected((u32, BlockHash))
            let mut disconnected: VecDeque<BlockHeader> = cache.rollback_active_chain(common_ancestor).await?;
            while !disconnected.is_empty() {
                if let Some(block_header) = disconnected.pop_back() {
                    let block_ext: BlockExtended = http_client.get_block(block_header.block_hash()).await?;
                    yield BlockEvent::Disconnected((block_ext.height, block_header.block_hash()));
                }
            }

            // iterate over forked chain candidates
            // update [`Cache`] active_headers field with candidates
            let (_, mut connected) = cache.apply_fork_chain(fork_chain)?;
            while !connected.is_empty() {
                let block = connected.pop_back().unwrap();
                yield BlockEvent::Connected(BlockHeader::from(block.clone()));
            }
        }
    };
    Ok(stream)
}

/// Subscribe to a real-time stream of [`BlockEvent`], for all new blocks or starting from an optional checkpoint
pub async fn subscribe_to_block_headers(
    http_base_url: &str,
    ws_base_url: &str,
    checkpoint: Option<(u32, BlockHash)>,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<BlockEvent<BlockHeader>>>>>> {
    match checkpoint {
        Some(checkpoint) => {
            let headers_cache =
                BlockHeadersCache::new_with_checkpoint(http_base_url, checkpoint).await?;
            let prev_headers = fetch_block_headers(http_base_url, checkpoint).await?;
            let new_headers = websocket::listen_new_block_headers(ws_base_url).await?;
            let candidates = Box::pin(prev_headers.chain(new_headers));
            Ok(Box::pin(
                process_candidates(http_base_url, headers_cache, candidates).await?,
            ))
        }
        None => {
            let headers_cache = BlockHeadersCache::new(http_base_url).await?;
            let new_header_candidates =
                Box::pin(websocket::listen_new_block_headers(ws_base_url).await?);
            Ok(Box::pin(
                process_candidates(http_base_url, headers_cache, new_header_candidates).await?,
            ))
        }
    }
}

/// Subscribe to a real-time stream of [`BlockEvent`] of full rust-bitcoin blocks, for new mined blocks or
/// starting from an optional checkpoint (height: u32, hash: BlockHash)
pub async fn subscribe_to_blocks(
    http_base_url: &str,
    ws_base_url: &str,
    checkpoint: Option<(u32, BlockHash)>,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<BlockEvent<Block>>>> {
    // build and create a http client
    let http_client = HttpClient::new(http_base_url);

    // subscribe to block_headers events
    let mut header_events =
        subscribe_to_block_headers(http_base_url, ws_base_url, checkpoint).await?;

    // iterate through each event for block_headers
    let stream = try_stream! {
        while let Some(event) = header_events.next().await {
            match event? {
                BlockEvent::Connected(header) => {
                // fetch all transaction ids (Txids) for the block
                let tx_ids = http_client.get_tx_ids(header.block_hash()).await?;

                // fetch full transaction and build transaction list Vec<Transaction>
                let mut txs: Vec<Transaction> = Vec::new();
                for id in tx_ids {
                    let tx = http_client.get_tx(id).await?;
                    txs.push(Transaction::from(tx));
                }

                // yield connected event for full block
                yield BlockEvent::Connected(Block {
                    header: header,
                    txdata: txs,
                });
            },
                // otherwise yield error or the disconnected event
                BlockEvent::Disconnected((height, hash)) => yield BlockEvent::Disconnected((height, hash)),
            }
        }
    };
    Ok(stream)
}

/// Fetch all [`BlockHeader`] starting from the checkpoint ([`u32`], [`BlockHeader`]) up to tip
pub async fn fetch_block_headers(
    base_url: &str,
    checkpoint: (u32, BlockHash),
) -> anyhow::Result<impl TryStream<Item = anyhow::Result<BlockHeader>>> {
    let http_client = HttpClient::new(base_url);

    // checks if the checkpoint height and hash matches for the current chain
    let (ckpt_height, ckpt_hash) = checkpoint;
    if ckpt_hash != http_client.get_block_hash(ckpt_height).await? {
        return Err(anyhow!(
            "The checkpoint passed is invalid, it should exist in the blockchain."
        ));
    }

    let tip_height = http_client.get_tip_height().await?;
    let stream = try_stream! {
        for height in ckpt_height..tip_height {
            let block_hash = http_client.get_block_hash(height).await?;
            let block_header = http_client.get_block_header(block_hash).await?;
            yield block_header;
        };

        let tip_hash = http_client.get_tip_hash().await?;
        let tip_header = http_client.get_block_header(tip_hash).await?;
        yield tip_header;
    };
    Ok(stream)
}
