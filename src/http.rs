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

use bitcoin::{consensus::deserialize, hashes::hex::FromHex, Block, BlockHash, Transaction, Txid};
use reqwest::Client;

use crate::api::BlockExtended;

#[cfg(feature = "tls-secure")]
static HTTP_PROTOCOL: &str = "https";

#[cfg(not(feature = "tls-secure"))]
static HTTP_PROTOCOL: &str = "http";

#[cfg(feature = "esplora-backend")]
static API_PREFIX: &str = "api";

#[cfg(feature = "mempool-backend")]
static API_PREFIX: &str = "api/v1";

/// Generic HttpClient using `reqwest`
/// It has been based on the Esplora client from BDK
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HttpClient {
    url: String,
    client: Client,
    concurrency: u8,
}

impl HttpClient {
    /// Creates a new HttpClient, for given base url and concurrency
    pub fn new(base_url: &str, concurrency: u8) -> Self {
        let url =
            url::Url::parse(format!("{}://{}/{}", HTTP_PROTOCOL, base_url, API_PREFIX).as_str())
                .unwrap();
        HttpClient {
            url: url.to_string(),
            client: Client::new(),
            concurrency,
        }
    }

    /// Get current blockchain block height (the current tip height)
    pub async fn _get_tip_height(&self) -> anyhow::Result<u32> {
        let res = self
            .client
            .get(&format!("{}/blocks/tip/height", self.url))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get [`BlockHash`] from mempool.space, for current tip
    pub async fn _get_tip_hash(&self) -> anyhow::Result<BlockHash> {
        let res = self
            .client
            .get(&format!("{}/blocks/tip/hash", self.url))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get [`BlockHash`] from mempool.space, for given block height
    pub async fn _get_block_hash(&self, height: u32) -> anyhow::Result<BlockHash> {
        let res = self
            .client
            .get(&format!("{}/block-height/{}", self.url, height))
            .send()
            .await?;

        Ok(res.error_for_status()?.text().await?.parse()?)
    }

    /// Get [`BlockExtended`] from mempool.space, by [`BlockHash`]
    pub async fn _get_block(&self, block_hash: BlockHash) -> anyhow::Result<BlockExtended> {
        let res = self
            .client
            .get(&format!("{}/block/{}", self.url, block_hash))
            .send()
            .await?;

        Ok(serde_json::from_str(
            res.error_for_status()?.text().await?.as_str(),
        )?)
    }

    /// FIXME: (@leonardo.lima) this only works when using the blockstream.info (esplora) client
    #[cfg(feature = "esplora-backend")]
    pub async fn _get_block_raw(&self, block_hash: BlockHash) -> anyhow::Result<Block> {
        let res = self
            .client
            .get(&format!("{}/block/{}/raw", self.url, block_hash))
            .send()
            .await?;

        let block: Block = deserialize(res.bytes().await?.deref())?;

        Ok(block)
    }

    pub async fn _get_tx(&self, tx_id: Txid) -> anyhow::Result<Transaction> {
        let res = self
            .client
            .get(&format!("{}/tx/{}/hex", self.url, tx_id))
            .send()
            .await?;

        let tx: Transaction = deserialize(&Vec::<u8>::from_hex(
            res.error_for_status()?.text().await?.as_str(),
        )?)?;

        Ok(tx)
    }

    pub async fn _get_tx_ids(&self, block_hash: BlockHash) -> anyhow::Result<Vec<Txid>> {
        let res = self
            .client
            .get(format!("{}/block/{}/txids", self.url, block_hash))
            .send()
            .await?;

        let tx_ids: Vec<Txid> = serde_json::from_str(res.text().await?.as_str())?;
        Ok(tx_ids)
    }
}
