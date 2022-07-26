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

use bitcoin::{Address, BlockHash, BlockHeader, TxMerkleNode};

/// A structure that implements the equivalent `BlockExtended` type from mempool.space,
/// which is expected and parsed as response
#[derive(serde::Deserialize, Clone, Debug, Copy)]
pub struct BlockExtended {
    pub id: BlockHash,
    pub height: u32,
    pub version: i32,
    // none for genesis block
    #[serde(alias = "previousblockhash")]
    pub prev_blockhash: BlockHash,
    pub merkle_root: TxMerkleNode,
    #[serde(alias = "timestamp")]
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
    // add new fields if needed
}

// FIXME: (@leonardo.lima) Should this use serde_json or some other approach instead ?
impl From<BlockExtended> for BlockHeader {
    fn from(extended: BlockExtended) -> BlockHeader {
        BlockHeader {
            version: (extended.version),
            prev_blockhash: (extended.prev_blockhash),
            merkle_root: (extended.merkle_root),
            time: (extended.time),
            bits: (extended.bits),
            nonce: (extended.nonce),
        }
    }
}

/// Structure that implements the standard mempool.space WebSocket client response message
#[derive(serde::Deserialize, Debug)]
pub struct MempoolSpaceWebSocketMessage {
    pub block: BlockExtended,
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
