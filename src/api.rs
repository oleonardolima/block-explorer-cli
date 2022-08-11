// Block Events Library
// Written in 2022 by Leonardo Lima <> and Lloyd Fournier <>
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! All structs from mempool.space API
//! Also contains the main [`BlockEvent`]

use bitcoin::{Address, Block, BlockHash, BlockHeader, TxMerkleNode};

/// A structure that implements the equivalent `BlockExtended` type from mempool.space,
/// which is expected and parsed as response
#[derive(serde::Deserialize, Clone, Debug, Copy)]
pub struct BlockExtended {
    pub id: BlockHash,
    pub height: u32,
    pub version: i32,
    #[serde(alias = "previousblockhash")]
    pub prev_blockhash: Option<BlockHash>, // None for genesis block
    pub merkle_root: TxMerkleNode,
    #[serde(alias = "timestamp")]
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}

impl From<BlockExtended> for BlockHeader {
    fn from(extended: BlockExtended) -> Self {
        BlockHeader {
            version: extended.version,
            prev_blockhash: extended.prev_blockhash.unwrap_or_default(),
            merkle_root: extended.merkle_root,
            time: extended.time,
            bits: extended.bits,
            nonce: extended.nonce,
        }
    }
}

impl From<Block> for BlockExtended {
    fn from(block: Block) -> Self {
        BlockExtended {
            id: block.block_hash(),
            height: block
                .bip34_block_height()
                .expect("Given `bitcoin::Block` does not have height encoded as bip34")
                as u32,
            version: block.header.version,
            prev_blockhash: Some(block.header.prev_blockhash),
            merkle_root: block.header.merkle_root,
            time: block.header.time,
            bits: block.header.bits,
            nonce: block.header.nonce,
        }
    }
}

/// Structure that implements the standard mempool.space WebSocket client response message
#[derive(serde::Deserialize, Debug)]
pub struct MempoolSpaceWebSocketMessage {
    pub block: BlockExtended,
    // pub mempool_info: MempoolInfo,
    // pub da: DifficultyAdjustment,
    // pub fees: RecommendedFee,
}

/// Structure that implements the standard fields for mempool.space WebSocket client message
#[derive(serde::Serialize, Debug)]
pub struct MempoolSpaceWebSocketRequestMessage {
    pub action: String,
    pub data: Vec<String>,
}

/// Enum that implements the candidates for first message request for mempool.space WebSocket client
#[allow(dead_code)]
pub enum MempoolSpaceWebSocketRequestData {
    /// Used to listen only new blocks
    Blocks,
    /// Used to subscribe to mempool-blocks events
    MempoolBlocks,
    /// Used to subscribe to all events related to an address
    TrackAddress(Address),
}

/// Enum that implements the variants for `BlockEvent`
#[derive(Debug, Clone, Copy)]
pub enum BlockEvent<T> {
    /// Used when connecting and extending the current active chain being streamed
    Connected(T),
    /// Used when there is a fork or reorganization event that turns the block stale
    /// then it's disconnected from current active chain
    Disconnected((u32, BlockHash)),
}
