use anyhow::anyhow;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use parity_scale_codec::{Decode, Encode};
use runtime::{MmrParaA, Block, Header, BlockNumber, Hash};
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
const DEFAULT_ENDPOINT_PARA_SENDER: &str = "ws://localhost:54888";
const DEFAULT_RPC_ENDPOINT_PARA_SENDER: &str = "http://localhost:54888";
const DEFAULT_ENDPOINT_PARA_RECEIVER: &str = "ws://localhost:54887";
const DEFAULT_RPC_ENDPOINT_PARA_RECEIVER: &str = "http://localhost:54887";
const DEFAULT_ENDPOINT_RELAY: &str = "ws://localhost:54886";
const DEFAULT_RPC_ENDPOINT_RELAY: &str = "http://localhost:54886";

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

	let _ = log_all_mmr_roots(&para_sender_api).await?;
	let _ = log_all_mmr_proofs(&para_sender_api).await?;
	let _ = get_proof_and_verify(&para_sender_api).await?;

	let subscribe = log_all_blocks(&vec![para_sender_api, para_receiver_api, relay_api]).await?;

	std::future::pending::<()>().await;
	Ok(())
}

async fn get_proof_and_verify(client: &MultiClient) -> anyhow::Result<()> {
	let root = generate_mmr_root(&client).await?;
	let proof = generate_mmr_proof(&client).await?;
	let params = rpc_params![root, proof];

	let request: Option<bool> = client.rpc_client.request("mmr_verifyProofStateless", params).await?;
	let verification = request.ok_or(RelayerError::Default)?;
	log::info!("Was proof verified? Answer:: {}", verification);
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

async fn log_all_mmr_roots(client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		let para_id = 0u64;

		while let Some(block) = blocks_sub.next().await {
			let block = block?;
			let params = rpc_params![Option::<Hash>::None, para_id];
			let request: Option<Hash> = client.rpc_client.request("mmr_root", params).await?;
			let hash = request.ok_or(RelayerError::Default)?;
			log::info!("Mmr root for block_hash {:?}, block_number {:?} root {:?}", block.hash(), block.number(), hash);
		}

		Ok::<(), anyhow::Error>(())
	});

	Ok(())
}

async fn log_all_mmr_proofs(client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		let para_id = 0u64;
		let mut proof_sizes = Vec::<u64>::new();
		let mut mmr_blocks_to_query = vec![1u64];
		let mut mmr_blocks_benchmark_counter = 1u64;

		// benchmark
		// growable vec that after some delay time adds another index to the vec and makes the rpc request.
		// and takes the length of the proof vec and sees the average difference from the previous proof vec len
		// and the current. So just stick proof len minus previous proof len in a vector
		// and then log the mean(sum over all elements and divide by num elems)

		while let Some(block) = blocks_sub.next().await {
			let block = block?;
			let params = rpc_params![mmr_blocks_to_query.clone(), Option::<BlockNumber>::None, Option::<Hash>::None, para_id];
			let request: Option<LeavesProof<H256>> = client.rpc_client.request("mmr_generateProof", params).await?;
			let proof = request.ok_or(RelayerError::Default)?;
			mmr_blocks_to_query.push(mmr_blocks_benchmark_counter);
			mmr_blocks_benchmark_counter += 1;

			let curr_proof_size = proof.proof.len() as u64;
			proof_sizes.push(curr_proof_size);
			let mean = do_mean(&proof_sizes).await?;
			log::info!("num of mmr blocks: {}, counter value: {}, proof_size: {}, curr_avg_proof_size: {}", mmr_blocks_to_query.len(), mmr_blocks_benchmark_counter, curr_proof_size, mean);
			log::info!("Mmr proof for block_hash {:?}, block_number {:?}, proof {:?}", block.hash(), block.number(), proof);
		}

		Ok::<(), anyhow::Error>(())
	});

	Ok(())
}

async fn do_mean(vec: &[u64]) -> anyhow::Result<u64> {
	let mut sum = 0u64;
	let mut len = vec.len() as u64;
	vec.iter().for_each(|difference| sum += difference);
	Ok(sum / len)
}

// Call generate mmr proof for sender
async fn generate_mmr_proof(client: &MultiClient) -> anyhow::Result<LeavesProof<H256>> {
	let block = client.subxt_client.blocks().at_latest().await?;

	let params = rpc_params![vec![1,2,3,4], Option::<BlockNumber>::None, Option::<Hash>::None, 0u64];

	let request: Option<LeavesProof<H256>> = client.rpc_client.request("mmr_generateProof", params).await?;
	let proof = request.ok_or(RelayerError::Default)?;
	log::info!("Proof obtained:: {:?}", proof);
	Ok(proof)
}

// Call generate mmr proof for sender
async fn generate_mmr_root(client: &MultiClient) -> anyhow::Result<H256> {
	let block = client.subxt_client.blocks().at_latest().await?;

	let params = rpc_params![Option::<Hash>::None, 0u64];

	let request: Option<Hash> = client.rpc_client.request("mmr_root", params).await?;
	let root = request.ok_or(RelayerError::Default)?;
	log::info!("root obtained:: {:?}", root);
	Ok(root)
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