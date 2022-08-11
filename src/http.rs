// Block Events Library
// Written in 2022 by Leonardo Lima <> and Lloyd Fournier <>
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! Http client implementation for mempool.space available endpoints
//! It used `reqwest` async client

#![allow(unused_imports)]
use std::ops::Deref;

use bitcoin::{
    consensus::deserialize, hashes::hex::FromHex, Block, BlockHash, BlockHeader, Transaction, Txid,
};
use reqwest::Client;

use crate::api::BlockExtended;

/// Generic HttpClient using `reqwest`
///
/// This implementation and approach is based on the BDK's esplora client
///
/// `<https://github.com/bitcoindevkit/bdk/blob/master/src/blockchain/esplora/reqwest.rs>`
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HttpClient {
    /// The base url for building our http rest calls
    /// It's expected to have the protocol, domain and initial api path (e.g: `<http://mempool.space/api/v1>`)
    base_url: String,
    /// A `reqwest` client with default or selected config
    client: Client,
    /// The number of concurrency requests the client is allowed to make
    concurrency: u8,
}

impl HttpClient {
    /// Creates a new HttpClient, for given base url and concurrency
    pub fn new(base_url: &str) -> Self {
        HttpClient {
            client: Client::new(),
            base_url: base_url.to_string(),
            concurrency: crate::DEFAULT_CONCURRENT_REQUESTS,
        }
    }

    /// Get current blockchain height [`u32`], the current tip height
    pub async fn get_tip_height(&self) -> anyhow::Result<u32> {
        let res = self
            .client
            .get(&format!("{}/blocks/tip/height", self.base_url))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get current blockchain hash [`BlockHash`], the current tip hash
    pub async fn get_tip_hash(&self) -> anyhow::Result<BlockHash> {
        let res = self
            .client
            .get(&format!("{}/blocks/tip/hash", self.base_url))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get the [`BlockHash`] for given block height
    pub async fn get_block_hash(&self, height: u32) -> anyhow::Result<BlockHash> {
        let res = self
            .client
            .get(&format!("{}/block-height/{}", self.base_url, height))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get the [`BlockHeader`] for given [`BlockHash`]
    pub async fn get_block_header(&self, hash: BlockHash) -> anyhow::Result<BlockHeader> {
        let res = self
            .client
            .get(&format!("{}/block/{}/header", self.base_url, hash))
            .send()
            .await?;

        let raw_header = Vec::<u8>::from_hex(res.error_for_status()?.text().await?.as_str())?;
        let header: BlockHeader = deserialize(&raw_header)?;

        Ok(header)
    }

    /// Get full block in [`BlockExtended`] format, for given [`BlockHash`]
    pub async fn get_block(&self, block_hash: BlockHash) -> anyhow::Result<BlockExtended> {
        let res = self
            .client
            .get(&format!("{}/block/{}", self.base_url, block_hash))
            .send()
            .await?;

        Ok(serde_json::from_str(
            res.error_for_status()?.text().await?.as_str(),
        )?)
    }

    /// This only works when using the blockstream.info (esplora) client, it does not work with mempool.space client
    ///
    /// NOTE: It will be used instead of multiple calls for building blocks as this commit is added in a new mempool.space
    /// release: `<https://github.com/mempool/mempool/commit/b2e657374350045ed2fad282a73b7b0c7975376f>`
    pub async fn get_block_raw(&self, block_hash: BlockHash) -> anyhow::Result<Block> {
        let res = self
            .client
            .get(&format!("{}/block/{}/raw", self.base_url, block_hash))
            .send()
            .await?;

        let block: Block = deserialize(res.bytes().await?.deref())?;

        Ok(block)
    }

    /// Get all transactions ids [`Vec<Txid>`] for given [`BlockHash`]
    pub async fn get_tx_ids(&self, block_hash: BlockHash) -> anyhow::Result<Vec<Txid>> {
        let res = self
            .client
            .get(format!("{}/block/{}/txids", self.base_url, block_hash))
            .send()
            .await?;

        let tx_ids: Vec<Txid> = serde_json::from_str(res.text().await?.as_str())?;
        Ok(tx_ids)
    }

    /// Get the [`Transaction`] for given transaction hash/id [`Txid`]
    pub async fn get_tx(&self, tx_id: Txid) -> anyhow::Result<Transaction> {
        let res = self
            .client
            .get(&format!("{}/tx/{}/hex", self.base_url, tx_id))
            .send()
            .await?;

        let raw_tx = Vec::<u8>::from_hex(res.error_for_status()?.text().await?.as_str())?;
        let tx: Transaction = deserialize(&raw_tx)?;

        Ok(tx)
    }
}
