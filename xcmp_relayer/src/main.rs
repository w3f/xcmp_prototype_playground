use anyhow::anyhow;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use parity_scale_codec::{Decode, Encode};
use runtime::{MmrParaA, Block, Header};
use std::path::PathBuf;

use sp_runtime::traits::Keccak256;

use sp_core::H256;

use tokio::task;
use std::time::Duration;

use futures::StreamExt;
use subxt::{OnlineClient, PolkadotConfig, backend::rpc::{RpcClient, RpcClientT}};

use mmr_rpc::LeavesProof;

// use subxt_signer::sr25519::dev;

/// The default endpoints for each
const DEFAULT_ENDPOINT_PARA_SENDER: &str = "ws://localhost:9933";
const DEFAULT_RPC_ENDPOINT_PARA_SENDER: &str = "http://localhost:9933";
const DEFAULT_ENDPOINT_PARA_RECEIVER: &str = "ws://localhost:9944";
const DEFAULT_RPC_ENDPOINT_PARA_RECEIVER: &str = "http://localhost:9944";
const DEFAULT_ENDPOINT_RELAY: &str = "ws://localhost:9955";
const DEFAULT_RPC_ENDPOINT_RELAY: &str = "http://localhost:9955";

#[derive(Clone, Debug)]
pub struct MultiClient {
	pub subxt_client: OnlineClient<PolkadotConfig>,
	pub rpc_client: HttpClient,
}

impl MultiClient {
	pub async fn new(endpoint: &str, rpc_endpoint: &str) -> anyhow::Result<Self> {
		Ok(Self {
			subxt_client: OnlineClient::<PolkadotConfig>::from_url(endpoint).await?,
			rpc_client: HttpClientBuilder::default().build(rpc_endpoint)?,
		})
	}
}

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

	let para_sender_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_SENDER, DEFAULT_RPC_ENDPOINT_PARA_SENDER ).await?;
	let para_receiver_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_RECEIVER, DEFAULT_RPC_ENDPOINT_PARA_RECEIVER).await?;
	let relay_api = MultiClient::new(DEFAULT_ENDPOINT_RELAY, DEFAULT_RPC_ENDPOINT_RELAY).await?;

	let gen_mmr_proof = generate_mmr_proof(&para_sender_api).await?;

	let subscribe = log_all_blocks(&vec![para_sender_api, para_receiver_api, relay_api]).await?;

	std::future::pending::<()>().await;
	Ok(())
}

async fn log_all_blocks(clients: &[MultiClient]) -> anyhow::Result<()> {
	for (client_index, client) in clients.iter().enumerate() {
		let client = client.clone();
		
		task::spawn(async move {
			let client_type = ClientType::from_index(client_index);
			let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
			
			while let Some(block) = blocks_sub.next().await {
				let block = block?;
				
				let block_number = block.header().number;
				let block_hash = block.hash();
				
				log::info!("Block for {:?}: is block_hash {:?}, block_number {:?}",
					client_type, block.hash(), block.number());
			}

			Ok::<(), anyhow::Error>(())
		});
	}

	Ok(())
}

// Call generate mmr proof for sender
async fn generate_mmr_proof(client: &MultiClient) -> anyhow::Result<LeavesProof<Keccak256>> {
	let block = client.subxt_client.blocks().at_latest().await?;

	let params = rpc_params![vec![block.number()]];

	let request: Option<LeavesProof<Keccak256>> = client.rpc_client.request("mmr_generateProof", params).await?;
	let proof = request.ok_or(RelayerError::Default)?;
	Ok(proof)
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
