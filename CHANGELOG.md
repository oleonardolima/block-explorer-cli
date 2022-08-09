# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
- Add `block_events::websocket` client, and a `websocket::listen_new_block_headers` fn to get a stream of websocket messages.
- Add `block_events::http` client, and multiple fns to consume data from mempool.space and/or esplora REST APIs.
- Add `block_events::api` with multiple structs and definition for responses, such as: `BlockExtended` and events as `BlockEvent`
- Add `block_events::subscribe_to_block_headers` fn in order to return a `Stream` of `block_events::api::BlockEvent(bitcoin::BlockHeader)`.
- Add `block_events::process_blocks` to handle block reorganization, manage in-memory cache struct `BlockHeadersCache`, and propagate errors to caller.
- Add `block_events::subscribe_to_blocks` fn in order to return a `Stream` of `block_events::api::BlockEvent(bitcoin::Block)`.
