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

#[cfg(all(feature = "esplora-backend", feature = "mempool-backend"))]
compile_error!("features esplora-backend and mempool-backend are mutually exclusive and cannot be enabled together");

pub mod api;
pub mod http;
pub mod websocket;

pub extern crate async_stream;
pub extern crate bitcoin;
pub extern crate tokio;
pub extern crate tokio_stream;
pub extern crate tokio_tungstenite;

use std::{collections::HashMap, collections::VecDeque, pin::Pin};

use api::{BlockEvent, BlockExtended};
use futures_core::TryStream;
use http::HttpClient;

use anyhow::{anyhow, Ok};
use async_stream::{stream, try_stream};
use bitcoin::{Block, BlockHash, BlockHeader, Transaction};
use core::result::Result::Ok as CoreOk;
use futures_util::stream::Stream;
use tokio_stream::StreamExt;

const DEFAULT_CONCURRENT_REQUESTS: u8 = 4;

/// A simple cache struct to store the all fetched and new blocks in-memory
///
/// It's used in order to handle reorganization events, and produce both connected and disconnected events
#[derive(Debug, Clone)]
pub struct BlockHeadersCache {
    pub tip: BlockHash,
    pub active_headers: HashMap<BlockHash, BlockExtended>,
    pub stale_headers: HashMap<BlockHash, BlockExtended>,
}

impl BlockHeadersCache {
    /// Validate if the new [`BlockHeader`] or [`BlockExtended`] candidate is a valid tip
    ///
    /// Updates the [`BlockHeadersCache`] state and returns a boolean
    pub fn validate_new_header(&mut self, candidate: BlockExtended) -> bool {
        if self.tip == candidate.prev_blockhash {
            self.tip = candidate.id;
            self.active_headers.insert(candidate.id, candidate);
            return true;
        }
        false
    }

    /// Find common ancestor for current active chain and the fork chain candidate
    ///
    /// Updates the [`BlockHeadersCache`] state with fork chain candidates
    ///
    /// Returns a common ancestor [`BlockExtended`] stored in [`BlockHeadersCache`] and fork branch chain as a `VecDeque<BlockExtended>`
    pub async fn find_or_fetch_common_ancestor(
        &self,
        http_client: HttpClient,
        branch_candidate: BlockExtended,
    ) -> anyhow::Result<(BlockExtended, VecDeque<BlockExtended>)> {
        let mut common_ancestor = branch_candidate;
        let mut fork_branch: VecDeque<BlockExtended> = VecDeque::new();
        while !self.active_headers.contains_key(&common_ancestor.id) {
            fork_branch.push_back(common_ancestor);
            common_ancestor = http_client
                ._get_block(common_ancestor.prev_blockhash)
                .await?;
        }
        Ok((common_ancestor, fork_branch))
    }

    /// Rollback active chain in [`BlockHeadersCache`] back to passed block
    ///
    /// Returns all stale, and to be disconnected blocks as a `VecDeque<BlockExtended>`
    pub async fn rollback_active_chain(
        &mut self,
        block: BlockExtended,
    ) -> anyhow::Result<VecDeque<BlockExtended>> {
        let mut disconnected = VecDeque::new();
        while block.id != self.tip {
            let (stale_hash, stale_header) = self.active_headers.remove_entry(&self.tip).unwrap();
            disconnected.push_back(stale_header);

            self.stale_headers.insert(stale_hash, stale_header);
            self.tip = stale_header.prev_blockhash;
        }
        Ok(disconnected)
    }

    /// Apply fork branch to active chain, and update tip to new `BlockExtended`
    ///
    /// Returns the new tip `BlockHash`, and the connected blocks as a `VecDeque<BlockExtended>`
    pub fn apply_fork_chain(
        &mut self,
        mut fork_branch: VecDeque<BlockExtended>,
    ) -> anyhow::Result<(BlockHash, VecDeque<BlockExtended>)> {
        let mut connected = VecDeque::new();
        while !fork_branch.is_empty() {
            let block = fork_branch.pop_front().unwrap();
            connected.push_back(block);

            self.active_headers.insert(block.id, block);
            self.tip = block.id;
        }
        Ok((self.tip, connected))
    }
}

/// Subscribe to a real-time stream of [`BlockEvent`], for all new blocks or starting from an optional checkpoint
pub async fn subscribe_to_block_headers(
    base_url: &str,
    checkpoint: Option<(u32, BlockHash)>,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = BlockEvent<BlockHeader>>>>> {
    let http_client = http::HttpClient::new(base_url, DEFAULT_CONCURRENT_REQUESTS);

    let tip_height = match checkpoint {
        Some((height, _)) => height - 1,
        _ => http_client._get_tip_height().await?,
    };

    let cache = BlockHeadersCache {
        tip: http_client._get_block_hash(tip_height).await?,
        active_headers: HashMap::new(),
        stale_headers: HashMap::new(),
    };

    match checkpoint {
        Some(checkpoint) => {
            let prev_header_candidates = fetch_blocks(base_url, checkpoint).await?;
            let new_header_candidates = websocket::subscribe_to_block_headers(base_url).await?;
            let candidates = Box::pin(prev_header_candidates.chain(new_header_candidates));
            Ok(Box::pin(
                process_candidates(base_url, cache, candidates).await?,
            ))
        }
        None => {
            let new_header_candidates =
                Box::pin(websocket::subscribe_to_block_headers(base_url).await?);
            Ok(Box::pin(
                process_candidates(base_url, cache, new_header_candidates).await?,
            ))
        }
    }
}

/// Subscribe to a real-time stream of [`BlockEvent`] of full rust-bitcoin blocks, for new mined blocks or
/// starting from an optional checkpoint (height: u32, hash: BlockHash)
pub async fn subscribe_to_blocks(
    base_url: &str,
    checkpoint: Option<(u32, BlockHash)>,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<BlockEvent<Block>>>> {
    // build and create a http client
    let http_client = http::HttpClient::new(base_url, DEFAULT_CONCURRENT_REQUESTS);

    // subscribe to block_headers events
    let mut header_events = subscribe_to_block_headers(base_url, checkpoint).await?;

    // iterate through each event for block_headers
    let stream = try_stream! {
        while let Some(event) = header_events.next().await {
            match event {
                BlockEvent::Connected(header) => {
                // fetch all transaction ids (Txids) for the block
                let tx_ids = http_client._get_tx_ids(header.block_hash()).await?;

                // fetch full transaction and build transaction list Vec<Transaction>
                let mut txs: Vec<Transaction> = Vec::new();
                for id in tx_ids {
                    let tx = http_client._get_tx(id).await?;
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
    mut candidates: Pin<Box<dyn Stream<Item = anyhow::Result<BlockExtended>>>>,
) -> anyhow::Result<impl Stream<Item = BlockEvent<BlockHeader>>> {
    let http_client = HttpClient::new(base_url, DEFAULT_CONCURRENT_REQUESTS);

    let stream = stream! {
        while let Some(candidate) = candidates.next().await {
            match candidate {
                // TODO: (@leonardo.lima) We should handle and return an specific error for the client
                Err(_) => {/* ignore */},
                CoreOk(candidate) => {
                    // TODO: (@leonardo.lima) It should check and validate for valid BlockHeaders

                    // validate if the [`BlockHeader`] candidate is a valid new tip
                    // yields a [`BlockEvent::Connected()`] variant and continue the iteration
                    if cache.validate_new_header(candidate) {
                        yield BlockEvent::Connected(BlockHeader::from(candidate.clone()));
                        continue
                    }

                    // find common ancestor for current active chain and the forked chain
                    // fetches forked chain candidates and store in cache
                    let (common_ancestor, fork_chain) = cache.find_or_fetch_common_ancestor(http_client.clone(), candidate).await.unwrap();

                    // rollback current active chain, moving blocks to staled field
                    // yields BlockEvent::Disconnected((u32, BlockHash))
                    let mut disconnected: VecDeque<BlockExtended> = cache.rollback_active_chain(common_ancestor).await.unwrap();
                    while !disconnected.is_empty() {
                        let block: BlockExtended = disconnected.pop_back().unwrap();
                        yield BlockEvent::Disconnected((block.height, block.id));
                    }

                    // iterate over forked chain candidates
                    // update [`Cache`] active_headers field with candidates
                    let (_, mut connected) = cache.apply_fork_chain(fork_chain).unwrap();
                    while !connected.is_empty() {
                        let block = connected.pop_back().unwrap();
                        yield BlockEvent::Connected(BlockHeader::from(block.clone()));
                    }

                },
            }
        }
    };
    Ok(stream)
}

/// Fetch all new starting from the checkpoint up to current active tip
// FIXME: this fails when checkpoint is genesis block as it does not have a previousblockhash field
pub async fn fetch_blocks(
    base_url: &str,
    checkpoint: (u32, BlockHash),
) -> anyhow::Result<impl TryStream<Item = anyhow::Result<BlockExtended>>> {
    let http_client = HttpClient::new(base_url, DEFAULT_CONCURRENT_REQUESTS);

    // checks if the checkpoint height and hash matches for the current chain
    let (ckpt_height, ckpt_hash) = checkpoint;
    if ckpt_hash != http_client._get_block_hash(ckpt_height).await? {
        return Err(anyhow!(
            "The checkpoint passed is invalid, it should exist in the blockchain."
        ));
    }

    let tip_height = http_client._get_tip_height().await?;
    let stream = try_stream! {
        for height in ckpt_height..tip_height {
            let hash = http_client._get_block_hash(height).await?;
            let block = http_client._get_block(hash).await?;

            yield block;
        };

        let height = http_client._get_tip_height().await?;
        let hash = http_client._get_block_hash(height).await?;
        let block = http_client._get_block(hash).await?;
        yield block;
    };
    Ok(stream)
}
