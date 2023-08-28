use anyhow::anyhow;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use parity_scale_codec::{Decode, Encode};
use runtime::{MmrParaA, Block, Header};
use std::path::PathBuf;

use sp_core::H256;

use tokio::task;
use std::time::Duration;

/// The default RPC endpoints for each
const DEFAULT_ENDPOINT_PARA_SENDER: &str = "http://localhost:9933";
const DEFAULT_ENDPOINT_PARA_RECEIVER: &str = "http://localhost:9944";
const DEFAULT_ENDPOINT_RELAY: &str = "http://localhost:9955";

#[derive(Debug)]
pub enum ClientType {
	Sender,
	Receiver,
	Relay,
}

impl std::fmt::Display for ClientType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientType {:?}", self)
    }
}

impl ClientType {
	fn from_index(index: usize) -> Self {
		match index {
			0 => ClientType::Sender,
			1 => ClientType::Receiver,
			2 => ClientType::Relay,
			_ => panic!(),
		}
	}
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	log::info!("Booting up");

	let para_sender_client = HttpClientBuilder::default().build(DEFAULT_ENDPOINT_PARA_SENDER)?;
	let para_receiver_client = HttpClientBuilder::default().build(DEFAULT_ENDPOINT_PARA_RECEIVER)?;
	let relay_client = HttpClientBuilder::default().build(DEFAULT_ENDPOINT_RELAY)?;

	// TODO: Add the rest of the clients and print to validate
	let subscribe = subscribe_all_heads(&vec![para_sender_client]).await?;

	std::future::pending::<()>().await;
	Ok(())
}

/// For now just print all heads from all clients
async fn subscribe_all_heads(clients: &[HttpClient]) -> anyhow::Result<()> {
	// Call correct RPC for each client and print headers
	for (client_index, client) in clients.iter().enumerate() {
		let client = client.clone();

		task::spawn(async move {
			let client_type = ClientType::from_index(client_index);

			let genesis_hash = node_get_block_hash(0, &client).await?.ok_or(RelayerError::Default)?;
			log::info!("got genesis hash {:?}", genesis_hash);

			let mut head = node_get_block(Some(genesis_hash), &client).await?.ok_or(RelayerError::Default)?.header;
			log::info!("got genesis head {:?}", head);

			loop {
				tokio::time::sleep(Duration::from_secs(5)).await;

				let curr_head = node_get_block(None, &client).await?.ok_or(RelayerError::Default)?.header;
				if head != curr_head {
					log::info!("Block Header for {:?} is {:?}", client_type, curr_head);
				}
				head = curr_head;
			}
			Ok::<(), anyhow::Error>(())
		});
	}

	Ok(())
}

/// Typed helper to get the Node's block hash at a particular height
async fn node_get_block_hash(height: u32, client: &HttpClient) -> anyhow::Result<Option<H256>> {
    let params = rpc_params![Some(height)];
    let rpc_response: Option<String> = client.request("chain_getBlockHash", params).await?;
    let maybe_hash = rpc_response.map(|s| crate::h256_from_string(&s).unwrap());
    Ok(maybe_hash)
}

/// Parse a string into an H256 that represents a public key
fn h256_from_string(s: &str) -> anyhow::Result<H256> {
    let s = strip_0x_prefix(s);

    let mut bytes: [u8; 32] = [0; 32];
    hex::decode_to_slice(s, &mut bytes as &mut [u8])
        .map_err(|_| RelayerError::Default)?;
    Ok(H256::from(bytes))
}

/// Typed helper to get the node's full block at a particular hash
async fn node_get_block(hash: Option<H256>, client: &HttpClient) -> anyhow::Result<Option<Block>> {
	let mut params = rpc_params![];
	if let Some(block_hash) = hash {
		let s = hex::encode(block_hash.0);
		params = rpc_params![s];
	}

    let rpc_response: Option<serde_json::Value> = client.request("chain_getBlock", params).await?;

    Ok(rpc_response
        .and_then(|value| value.get("block").cloned())
        .and_then(|maybe_block| serde_json::from_value(maybe_block).unwrap_or(None)))
}

/// Takes a string and checks for a 0x prefix. Returns a string without a 0x prefix.
fn strip_0x_prefix(s: &str) -> &str {
    if &s[..2] == "0x" {
        &s[2..]
    } else {
        s
    }
}

#[derive(Debug)]
pub enum RelayerError {
    Default,
}

impl std::fmt::Display for RelayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Relayer Error {:?}", self)
    }
}

impl std::error::Error for RelayerError { }
