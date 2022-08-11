use bitcoin::BlockHash;
use bitcoind::{bitcoincore_rpc::RpcApi, BitcoinD};
use block_events::{api::BlockEvent, http::HttpClient, websocket};
use futures::{pin_mut, StreamExt};
use serial_test::serial;
use std::{collections::VecDeque, ops::Deref, time::Duration};
use testcontainers::{clients, images, images::generic::GenericImage, RunnableImage};

const HOST_IP: &str = "127.0.0.1";

const MARIADB_NAME: &str = "mariadb";
const MARIADB_TAG: &str = "10.5.8";
const MARIADB_READY_CONDITION: &str = "mysqld: ready for connections.";

const MEMPOOL_BACKEND_NAME: &str = "mempool/backend";
const MEMPOOL_BACKEND_TAG: &str = "v2.4.1";
const MEMPOOL_BACKEND_READY_CONDITION: &str = "Mempool Server is running on port 8999";

// TODO: (@leonardo.lima) This should be derived instead, should we add it to bitcoind ?
const RPC_AUTH: &str = "mempool:3c417dbc7ccabb51d8e6fedc302288db$ed44e37a937e8706ea51bbc761df76e995fe92feff8751ce85feaea4c4ae80b1";

#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn docker_host_address() -> &'static str {
    "host.docker.internal"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_arch = "aarch64"))]
fn docker_host_address() -> &'static &str {
    "172.17.0.1"
}

pub struct MempoolTestClient {
    pub bitcoind: BitcoinD,
    pub mariadb_database: RunnableImage<GenericImage>,
    pub mempool_backend: RunnableImage<GenericImage>,
}

impl MempoolTestClient {
    fn start_bitcoind(bitcoind_exe: Option<String>) -> BitcoinD {
        let bitcoind_exe = bitcoind_exe.unwrap_or(bitcoind::downloaded_exe_path().ok().expect(
            "you should provide a bitcoind_exe parameter or specify a bitcoind version feature",
        ));

        log::debug!("launching bitcoind [bitcoind_exe {:?}]", bitcoind_exe);

        let mut conf = bitcoind::Conf::default();
        let rpc_auth = format!("-rpcauth={}", RPC_AUTH);
        let rpc_bind = format!("-rpcbind=0.0.0.0");
        conf.args.push(rpc_auth.as_str());
        conf.args.push(rpc_bind.as_str());
        conf.args.push("-txindex");
        conf.args.push("-server");

        let bitcoind = BitcoinD::with_conf(&bitcoind_exe, &conf).unwrap();

        log::debug!(
            "successfully launched bitcoind and generated initial coins [bitcoind_exe {:?}]",
            bitcoind_exe
        );

        bitcoind
    }

    fn start_database(name: Option<&str>, tag: Option<&str>) -> RunnableImage<GenericImage> {
        let name = name.unwrap_or(MARIADB_NAME);
        let tag = tag.unwrap_or(MARIADB_TAG);

        log::debug!(
            "creating image and starting container [name {}] [tag {}]",
            name,
            tag
        );

        let image = images::generic::GenericImage::new(name, tag).with_wait_for(
            testcontainers::core::WaitFor::StdErrMessage {
                message: MARIADB_READY_CONDITION.to_string(),
            },
        );

        let image = RunnableImage::from(image)
            .with_env_var(("MYSQL_DATABASE", "mempool"))
            .with_env_var(("MYSQL_USER", "mempool"))
            .with_env_var(("MYSQL_PASSWORD", "mempool"))
            .with_env_var(("MYSQL_ROOT_PASSWORD", "mempool"))
            .with_mapped_port((3306, 3306));

        log::debug!(
            "successfully created and started container [name {}] [tag {}]",
            name,
            tag
        );
        image
    }

    fn start_backend(
        name: Option<&str>,
        tag: Option<&str>,
        core: &BitcoinD,
    ) -> RunnableImage<GenericImage> {
        let name = name.unwrap_or(MEMPOOL_BACKEND_NAME);
        let tag = tag.unwrap_or(MEMPOOL_BACKEND_TAG);

        log::debug!(
            "creating image and starting container [name {}] [tag {}]",
            name,
            tag
        );

        let image = images::generic::GenericImage::new(name, tag).with_wait_for(
            testcontainers::core::WaitFor::StdErrMessage {
                message: MEMPOOL_BACKEND_READY_CONDITION.to_string(),
            },
        );

        let bitcoind_port = core.params.rpc_socket.port().to_string();

        println!("{}", docker_host_address().to_string());

        let image = RunnableImage::from(image)
            .with_env_var(("MEMPOOL_BACKEND", "none"))
            .with_env_var(("DATABASE_HOST", docker_host_address().to_string()))
            .with_env_var(("CORE_RPC_HOST", docker_host_address().to_string()))
            .with_env_var(("CORE_RPC_PORT", bitcoind_port))
            .with_mapped_port((8999, 8999));

        log::debug!(
            "successfully created and started container [name {}] [tag {}]",
            name,
            tag
        );
        image
    }
}

impl Default for MempoolTestClient {
    fn default() -> Self {
        let bitcoind = Self::start_bitcoind(None);
        let mariadb = Self::start_database(None, None);
        let mempool = Self::start_backend(None, None, &bitcoind);

        MempoolTestClient {
            bitcoind: bitcoind,
            mariadb_database: mariadb,
            mempool_backend: mempool,
        }
    }
}

#[tokio::test]
#[serial]
async fn test_fetch_tip_height() {
    let _ = env_logger::try_init();

    let MempoolTestClient {
        bitcoind,
        mariadb_database,
        mempool_backend,
    } = MempoolTestClient::default();

    let docker = clients::Cli::docker();
    let _mariadb = docker.run(mariadb_database);

    // there is some small delay between running the docker for mariadb database and the port being really available
    std::thread::sleep(Duration::from_millis(5000));
    let mempool = docker.run(mempool_backend);

    let base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );

    let http_client = HttpClient::new(&base_url);

    // should return the current tip height
    for i in 0..5 {
        let tip = http_client.get_tip_height().await.unwrap();
        assert_eq!(i, tip);

        // generate new block
        let address = bitcoind.client.get_new_address(None, None).unwrap();
        let _ = bitcoind.client.generate_to_address(1, &address).unwrap();
    }
}

#[tokio::test]
#[serial]
async fn test_fetch_block_hash_by_height() {
    let _ = env_logger::try_init();
    let delay = Duration::from_millis(5000);

    let docker = clients::Cli::docker();
    let client = MempoolTestClient::default();

    let _mariadb = docker.run(client.mariadb_database);

    std::thread::sleep(delay); // there is some delay between running the docker and the port being really available
    let mempool = docker.run(client.mempool_backend);

    let rpc_client = &client.bitcoind.client;
    let base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );
    let http_client = HttpClient::new(&base_url);

    // should return an error if there is no block created yet for given height
    assert!(http_client.get_block_hash(100).await.is_err());

    // should return block hash for existing block by height
    for i in 1..10 {
        let gen_hash = rpc_client
            .generate_to_address(1, &rpc_client.get_new_address(None, None).unwrap())
            .unwrap();

        let res_hash = http_client.get_block_hash(i).await.unwrap();
        assert_eq!(gen_hash.first().unwrap(), &res_hash);
    }
}

#[tokio::test]
#[serial]
async fn test_fetch_blocks_for_invalid_checkpoint() {
    let _ = env_logger::try_init();
    let delay = Duration::from_millis(5000);

    let docker = clients::Cli::docker();
    let client = MempoolTestClient::default();

    let _mariadb = docker.run(client.mariadb_database);
    std::thread::sleep(delay); // there is some delay between running the docker and the port being really available

    let mempool = docker.run(client.mempool_backend);

    let base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );
    let checkpoint = (0, BlockHash::default());
    let blocks = block_events::fetch_block_headers(&base_url, checkpoint).await;

    // should produce an error for invalid checkpoint
    assert!(blocks.is_err());

    // should produce an error indicating checkpoint as invalid
    assert_eq!(
        blocks.err().unwrap().to_string(),
        "The checkpoint passed is invalid, it should exist in the blockchain."
    );
}

#[tokio::test]
#[serial]
async fn test_fetch_blocks_for_checkpoint() {
    let _ = env_logger::try_init();
    let delay = Duration::from_millis(5000);

    let docker = clients::Cli::docker();
    let client = MempoolTestClient::default();

    let _mariadb = docker.run(client.mariadb_database);
    std::thread::sleep(delay); // there is some delay between running the docker and the port being really available
    let mempool = docker.run(client.mempool_backend);

    let rpc_client = &client.bitcoind.client;

    // generate new 20 blocks
    let mut gen_blocks = rpc_client
        .generate_to_address(20, &rpc_client.get_new_address(None, None).unwrap())
        .unwrap();
    log::debug!("[{:#?}]", gen_blocks);

    let checkpoint = (10, *gen_blocks.get(9).unwrap());
    let base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );

    let blocks = block_events::fetch_block_headers(&base_url, checkpoint)
        .await
        .unwrap();

    pin_mut!(blocks);
    // should return all 10 blocks from 10 to 20, as 10 being the checkpoint
    for gen_block in &mut gen_blocks[9..] {
        let block = blocks.next().await.unwrap().unwrap();
        assert_eq!(gen_block.deref(), &block.block_hash());
    }
}

#[tokio::test]
#[serial]
async fn test_failure_for_invalid_websocket_url() {
    let block_events =
        websocket::listen_new_block_headers(&format!("ws://{}:{}", HOST_IP, 8999)).await;

    // should return an Err.
    assert!(block_events.is_err());

    // should return connection Err.
    assert_eq!(
        block_events.err().unwrap().to_string(),
        "IO error: Connection refused (os error 61)"
    );
}

#[tokio::test]
#[serial]
async fn test_block_events_stream() {
    let _ = env_logger::try_init();

    let MempoolTestClient {
        bitcoind,
        mariadb_database,
        mempool_backend,
    } = MempoolTestClient::default();

    let docker = clients::Cli::docker();
    let _mariadb = docker.run(mariadb_database);

    // there is some small delay between running the docker for mariadb database and the port being really available
    std::thread::sleep(Duration::from_millis(5000));
    let mempool = docker.run(mempool_backend);

    let http_base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );
    let ws_base_url = format!("ws://{}:{}", HOST_IP, mempool.get_host_port_ipv4(8999));

    // get stream of new block-events
    let events = block_events::subscribe_to_block_headers(&http_base_url, &ws_base_url, None)
        .await
        .unwrap();

    // generate 5 new blocks through bitcoind rpc-client
    let address = &bitcoind.client.get_new_address(None, None).unwrap();
    let mut blocks = VecDeque::from(bitcoind.client.generate_to_address(5, address).unwrap());

    // consume new blocks from block-events stream
    pin_mut!(events);
    while !blocks.is_empty() {
        let block_hash = blocks.pop_front().unwrap();
        let event = events.next().await.unwrap().unwrap();

        log::debug!("[event][{:#?}]", event);

        // should produce a BlockEvent::Connected result for each block event
        assert!(matches!(event, BlockEvent::Connected { .. }));

        // should handle and build the BlockEvent::Connected successfully
        let connected = match event {
            BlockEvent::Connected(header) => header,
            _ => unreachable!("This test is supposed to have only connected blocks, please check why it's generating disconnected and/or errors at the moment."),
        };

        assert_eq!(block_hash, connected.block_hash());
    }
}

#[tokio::test]
#[serial]
async fn test_block_events_stream_with_checkpoint() {
    let _ = env_logger::try_init();

    let MempoolTestClient {
        bitcoind,
        mariadb_database,
        mempool_backend,
    } = MempoolTestClient::default();

    // generate 10 new blocks through bitcoind rpc-client
    let address = &bitcoind.client.get_new_address(None, None).unwrap();
    let blocks = bitcoind.client.generate_to_address(10, address).unwrap();

    // expected blocks are from the 4th block in the chain (3rd in the new generated blocks)
    let mut expected_block_hashes = VecDeque::from(blocks[3..].to_vec());

    // checkpoint starts in 3rd new block (index 2)
    let ckpt_block_hash = *blocks.get(2).unwrap();
    let checkpoint = Some((3, ckpt_block_hash));

    // start database (mariadb) and backend (mempool/backend)
    let docker = clients::Cli::docker();
    let _mariadb = docker.run(mariadb_database);

    // there is some small delay between running the docker for mariadb database and the port being really available
    std::thread::sleep(Duration::from_millis(5000));
    let mempool = docker.run(mempool_backend);

    let http_base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );
    let ws_base_url = format!("ws://{}:{}", HOST_IP, mempool.get_host_port_ipv4(8999));

    // get block-events stream, starting from the checkpoint
    let block_events =
        block_events::subscribe_to_block_headers(&http_base_url, &ws_base_url, checkpoint)
            .await
            .unwrap();

    // consume new blocks from block-events stream
    pin_mut!(block_events);
    while !expected_block_hashes.is_empty() {
        let expected_hash = expected_block_hashes.pop_front().unwrap();
        let block_event = block_events.next().await.unwrap().unwrap();

        // should produce a BlockEvent::Connected result for each block event
        assert!(matches!(block_event, BlockEvent::Connected { .. }));

        // should parse the BlockEvent::Connected successfully
        let connected_hash = match block_event {
            BlockEvent::Connected(block) => block.block_hash(),
            _ => unreachable!("This test is supposed to have only connected blocks, please check why it's generating disconnected and/or errors at the moment."),
        };

        assert_eq!(expected_hash, connected_hash);
    }
}

#[tokio::test]
#[serial]
async fn test_block_events_stream_with_reorg() {
    let _ = env_logger::try_init();

    let MempoolTestClient {
        bitcoind,
        mariadb_database,
        mempool_backend,
    } = MempoolTestClient::default();

    // start database (mariadb) and backend (mempool/backend)
    let docker = clients::Cli::docker();
    let _mariadb = docker.run(mariadb_database);

    // there is some small delay between running the docker for mariadb database and the port being really available
    std::thread::sleep(Duration::from_millis(5000));
    let mempool = docker.run(mempool_backend);

    let http_base_url = format!(
        "http://{}:{}/api/v1",
        HOST_IP,
        mempool.get_host_port_ipv4(8999)
    );
    let ws_base_url = format!("ws://{}:{}", HOST_IP, mempool.get_host_port_ipv4(8999));

    // get block-events stream
    let block_events = block_events::subscribe_to_block_headers(&http_base_url, &ws_base_url, None)
        .await
        .unwrap();

    // generate 5 new blocks through bitcoind rpc-client
    let address = bitcoind.client.get_new_address(None, None).unwrap();
    let generated_blocks =
        VecDeque::from(bitcoind.client.generate_to_address(5, &address).unwrap());

    let mut new_blocks = generated_blocks.clone();

    // consume new blocks from block-events stream
    pin_mut!(block_events);
    while !new_blocks.is_empty() {
        let block_hash = new_blocks.pop_front().unwrap();
        let block_event = block_events.next().await.unwrap().unwrap();

        // should produce a BlockEvent::Connected result for each block event
        assert!(matches!(block_event, BlockEvent::Connected { .. }));

        // should parse the BlockEvent::Connected successfully
        let connected_block = match block_event {
            BlockEvent::Connected(block) => block,
            _ => unreachable!("This test is supposed to have only connected blocks, please check why it's generating disconnected and/or errors at the moment."),
        };
        assert_eq!(block_hash.to_owned(), connected_block.block_hash());
    }

    // invalidate last 2 blocks
    let mut invalidated_blocks = VecDeque::new();
    for block in generated_blocks.range(3..) {
        bitcoind.client.invalidate_block(block).unwrap();
        invalidated_blocks.push_front(block);
    }

    // generate 2 new blocks
    let address = bitcoind.client.get_new_address(None, None).unwrap();
    let mut new_blocks = VecDeque::from(bitcoind.client.generate_to_address(3, &address).unwrap());

    // should disconnect invalidated blocks
    while !invalidated_blocks.is_empty() {
        let invalidated = invalidated_blocks.pop_back().unwrap();
        let block_event = block_events.next().await.unwrap().unwrap();

        // should produce a BlockEvent::Connected result for each block event
        assert!(matches!(block_event, BlockEvent::Disconnected(..)));

        // should parse the BlockEvent::Connected successfully
        let disconnected = match block_event {
            BlockEvent::Disconnected((_, hash)) => hash,
            _ => unreachable!("This test is supposed to have only connected blocks, please check why it's generating disconnected and/or errors at the moment."),
        };
        assert_eq!(invalidated.to_owned(), disconnected);
    }

    // should connect the new created blocks
    while !new_blocks.is_empty() {
        let new_block = new_blocks.pop_front().unwrap();
        let block_event = block_events.next().await.unwrap().unwrap();

        // should produce a BlockEvent::Connected result for each block event
        assert!(matches!(block_event, BlockEvent::Connected { .. }));

        // should parse the BlockEvent::Connected successfully
        let connected = match block_event {
            BlockEvent::Connected(block) => block.block_hash(),
            _ => unreachable!("This test is supposed to have only connected blocks, please check why it's generating disconnected and/or errors at the moment."),
        };
        assert_eq!(new_block.to_owned(), connected);
    }
}
