use anyhow::anyhow;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use parity_scale_codec::{Decode, Encode};
use runtime::{Block, BlockNumber, Hash, Header, MmrParaA, UncheckedExtrinsic};
use std::{path::PathBuf, collections::BTreeMap};
use std::sync::Mutex;
use lazy_static::lazy_static;

use sp_runtime::traits::{
	Keccak256,
	Block as BlockT
};

use sp_core::H256;

use tokio::task;
use std::time::Duration;

use futures::StreamExt;
use subxt::{OnlineClient, PolkadotConfig, backend::rpc::{RpcClient, RpcClientT}};

use mmr_rpc::LeavesProof;
use sp_mmr_primitives::{Proof, EncodableOpaqueLeaf};

use subxt_signer::{sr25519::dev, ecdsa::dev::alice};

#[subxt::subxt(runtime_metadata_url = "ws://localhost:54887")]
pub mod polkadot { }

use polkadot::runtime_types::{
	pallet_xcmp_message_stuffer::XcmpProof,
	sp_mmr_primitives::Proof as XcmpProofType,
};

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

type BeefyMmrRoot = H256;
type RelayBlockIndex = u32;

lazy_static! {
	static ref BEEFY_MMR_MAP: Mutex<BTreeMap<BeefyMmrRoot, (RelayBlockIndex, LeavesProof<H256>)>> = Mutex::new(BTreeMap::new());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	log::info!("Booting up");

	let para_sender_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_SENDER, DEFAULT_RPC_ENDPOINT_PARA_SENDER ).await?;
	let para_receiver_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_RECEIVER, DEFAULT_RPC_ENDPOINT_PARA_RECEIVER).await?;
	let relay_api = MultiClient::new(DEFAULT_ENDPOINT_RELAY, DEFAULT_RPC_ENDPOINT_RELAY).await?;

	// Collect all Beefy Mmr Roots from Relaychain into a HashMap (BeefyMmrRoot -> Relaychain Block number)
	let _ = collect_relay_beefy_roots(&relay_api).await?;

	// TODO: Create mapping between Parablock Num -> Vec of all channel Mmr Roots for sender
	// Keep track of Mmr Index which correspondings to the receiver..
	// let _ = collect_all_mmr_roots_for_sender(&sender_api).await?;

	// TODO: Collect all ParaHeaders from sender in order to generate opening in
	// ParaHeader Merkle tree (ParaId, ParaHeader(As HeadData))
	// TODO: Add this as a RuntimeApi for generating this from the Relaychain directly
	// since data is stored there and proof can easily be generated from Validator
	// Then just Relay this over to receiver correctly in stage 2 of proof
	// How to convert ParaHeader -> HeadData? Just encode the header as bytes??

	let _ = log_all_mmr_roots(&para_sender_api).await?;
	let _ = log_all_mmr_proofs(&para_sender_api).await?;
	let _ = get_proof_and_verify(&para_sender_api, &relay_api).await?;

	let subscribe = log_all_blocks(&vec![para_sender_api, para_receiver_api, relay_api]).await?;

	std::future::pending::<()>().await;
	Ok(())
}

async fn collect_relay_beefy_roots(client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		while let Some(block) = blocks_sub.next().await {
			let block = block?;

			let params = rpc_params![Option::<Hash>::None, 0u64];
			log::info!("Before relay beefy root request");
			let request: Option<Hash> = match client.rpc_client.request("mmr_root", params).await {
				Ok(opt) => {
					log::info!("Relay MMR root request success");
					opt
				},
				Err(e) => {
					log::info!("Relay MMR root request failed with {:?}", e);
					None
				}
			};

			// let request: Option<Hash> = client.rpc_client.request("mmr_root", params).await?;
			log::info!("After relay beefy root request");
			let root = request.ok_or(RelayerError::Default)?;
			log::info!("Got Current Beefy Root from Relaychain {:?}", root);

			// let request: Option<Block> = client.rpc_client.request("chain_getBlock", rpc_params![Option::<Hash>::None]).await?;
			let request: Option<Header> = match client.rpc_client.request("chain_getHeader", rpc_params![Option::<Hash>::None]).await {
				Ok(block) => {
					block
				},
				Err(e) => {
					log::error!("Could not get block from chain with error {:?}", e);
					None
				}
			};
			log::info!("After chain block request");
			let rpc_header = request.ok_or(RelayerError::Default)?;
			if rpc_header.number != block.number() {
				log::error!(
					"RPC_Requested Block does not match Loop Block {} != {}",
					rpc_header.number,
					block.number()
				);
			}

			let proof = try_generate_mmr_proof(&client, block.number().into(), Some(0u64))
				.await
				.map_err(|e| {
					log::error!("Failed to generate big proof from Relaychain with error {:?}", e);
					RelayerError::Default
				}
			)?;

			match BEEFY_MMR_MAP.try_lock() {
				Ok(mut s) => {
					s.insert(root, (block.number(), proof));
					log::info!(
						"Inserting into storage Beefy root {:?}, for relay block number {}",
						root,
						block.number()
					)
				},
				Err(_) => log::error!("Could not lock BEEFY_MMR_MAP for writing")
			}

			log::info!("Beefy Mmr Root from Relaychain Obtained:: {:?}", root);
		}
		Ok::<(), anyhow::Error>(())
	});
	Ok(())
}

async fn generate_stage_1_proof(client: &MultiClient, relay_client: &MultiClient) -> anyhow::Result<()> {
	log::info!("Entered generate_stage_1_proof");
	let client = client.clone();
	// 1.) For each para block on receiver get the current Beefy Root on chain via RPC or subxt
	let beefy_api_call = polkadot::apis().messaging_api().get_current_beefy_root();
	let beefy_root = match client.subxt_client
		.runtime_api()
		.at_latest()
		.await?
		.call(beefy_api_call)
		.await {
			Ok(root) => {
				log::info!("Got root from `get_current_beefy_root` {:?}", root);
				root
			},
			Err(e) => {
				log::info!("Got error from trying to call `get_current_beefy_root` {:?}", e);
				H256::zero()
			}
		};
	log::info!("Beefy Root obtained in generate_stage_1_proof: {:?}", beefy_root);

	// 2.) Then call RPC to generate mmr_proof for Relay block number corresponding to this Beefy Root
	let (relay_block_num, proof) = match BEEFY_MMR_MAP.try_lock() {
		Ok(s) => {
			s.get(&beefy_root).cloned()
		},
		Err(_) => {
			log::info!("Could not lock BEEFY_MMR_MAP for reading");
			None
		}
	}.ok_or_else(|| {
			log::info!("Could not read relay_block_num and proof from BEEFY_MMR_MAP");
			RelayerError::Default
		}
	)?;

	log::info!("Got Relayblock Num {} for Beefy Root {:?}", relay_block_num, beefy_root);

	// 3.) Send transaction to chain for proof
	log::info!("calling submit_big_proof");
	submit_big_proof(&client, proof, beefy_root).await?;

	Ok(())
}

async fn update_root(client: &MultiClient, root: H256) -> anyhow::Result<()> {
	let signer = dev::alice();
	let channel_id = 0u64;
	let tx = crate::polkadot::tx().msg_stuffer_para_a().update_root(root, channel_id);

	let tx_progress = client.subxt_client.tx().sign_and_submit_then_watch_default(&tx, &signer).await?;
	let hash_tx = tx_progress.extrinsic_hash();
	match tx_progress.wait_for_in_block().await {
		Ok(tx_in_block) => {
			match tx_in_block.wait_for_success().await {
				Ok(events) => { log::info!("Got the tx in a block and it succeeded! {:?}", events); },
				Err(e) => { log::info!("Was not successful extrinsic ERROR:: {:?}", e); }
			}
		},
		Err(e) => {
			log::info!("Tx didnt get in a block error {:?}", e);
		}
	}
	log::info!("Hash of update_root: {:?}", hash_tx);
	Ok(())
}

async fn submit_big_proof(client: &MultiClient, proof: LeavesProof<H256>, beefy_root: H256) -> anyhow::Result<()> {
	let signer = dev::charlie();
	let leaves = Decode::decode(&mut &proof.leaves.0[..])
			.map_err(|e| anyhow::Error::new(e))?;
	let decoded_proof: XcmpProofType<H256> = Decode::decode(&mut &proof.proof.0[..])
			.map_err(|e| anyhow::Error::new(e))?;

	let dummy_proof: XcmpProofType<H256> = XcmpProofType::<H256> {
		leaf_indices: Vec::new(),
		leaf_count: 0u64,
		items: Vec::new(),
	};
	let xcmp_proof = XcmpProof {
		stage_1: (decoded_proof, leaves),
		stage_2: (),
		stage_3: (),
		// TODO: Remove. For now just testing stage 1 can pass
		stage_4: (dummy_proof, Vec::new()),
	};

	log::info!("Constructed Dummy Proof and XCMP Proof");

	let tx = crate::polkadot::tx().msg_stuffer_para_a().submit_big_proof(xcmp_proof, beefy_root);
	let tx_progress = client.subxt_client.tx().sign_and_submit_then_watch_default(&tx, &signer).await?;

	let hash_tx = tx_progress.extrinsic_hash();
	log::info!("Got After submitting submit_BIG_xcmp_proof");
	match tx_progress.wait_for_in_block().await {
		Ok(tx_in_block) => {
			match tx_in_block.wait_for_success().await {
				Ok(events) => { log::info!("Got the tx in a block and it succeeded! {:?}", events); },
				Err(e) => { log::info!("Was not successful extrinsic ERROR:: {:?}", e); }
			}
		},
		Err(e) => {
			log::info!("Tx didnt get in a block error {:?}", e);
		}
	}
	log::info!("Hash of BIG_xcmp_proof_submission: {:?}", hash_tx);

	Ok(())
}

async fn submit_proof(client: &MultiClient, proof: LeavesProof<H256>) -> anyhow::Result<()> {
	let signer = dev::bob();
	let channel_id = 0u64;
	let leaves = Decode::decode(&mut &proof.leaves.0[..])
					.map_err(|e| anyhow::Error::new(e))?;

	log::info!("Decoded leaves {:?}", leaves);

	let decoded_proof = Decode::decode(&mut &proof.proof.0[..])
			.map_err(|e| anyhow::Error::new(e))?;

	log::info!("Decoded proof {:?}", decoded_proof);

	let tx = crate::polkadot::tx().msg_stuffer_para_a().submit_xcmp_proof(decoded_proof, leaves, channel_id);

	let tx_progress = client.subxt_client.tx().sign_and_submit_then_watch_default(&tx, &signer).await?;
	let hash_tx = tx_progress.extrinsic_hash();
	log::info!("Got After submitting submit_xcmp_proof");
	match tx_progress.wait_for_in_block().await {
		Ok(tx_in_block) => {
			match tx_in_block.wait_for_success().await {
				Ok(events) => { log::info!("Got the tx in a block and it succeeded! {:?}", events); },
				Err(e) => { log::info!("Was not successful extrinsic ERROR:: {:?}", e); }
			}
		},
		Err(e) => {
			log::info!("Tx didnt get in a block error {:?}", e);
		}
	}
	log::info!("Hash of xcmp_proof_submission: {:?}", hash_tx);

	Ok(())
}

async fn get_proof_and_verify(client: &MultiClient, relay_client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	let relay_client = relay_client.clone();
	let channel_id = 0u64;
	let mut root = generate_mmr_root(&client, Some(0)).await?;
	let mut proof = generate_mmr_proof(&client, 1u64, Some(0)).await?;

	let params = rpc_params![root, proof.clone()];

	let request: Option<bool> = client.rpc_client.request("mmr_verifyProofStateless", params).await?;
	let verification = request.ok_or(RelayerError::Default)?;
	log::info!("Was proof verified? Answer:: {}", verification);

	let signer = dev::bob();

	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		while let Some(block) = blocks_sub.next().await {
			let block = block?;

			// check if current on chain root is equal to the original root to submit proof
			let onchain_root_query = crate::polkadot::storage().msg_stuffer_para_a().xcmp_channel_roots(0);
			let onchain_root = match client.subxt_client
			.storage()
			.at_latest()
			.await?
			.fetch(&onchain_root_query)
			.await {
				Ok(Some(root)) => root,
				Ok(None) => H256::zero(),
				Err(e) => H256::zero()
			};

			let update_root_string = match update_root(&client, root).await {
				Ok(_) => {
					"Updating root in loop".to_string()
				},
				Err(e) => format!("Cant update root on chain yet {:?}", e),
			};
			log::info!("{}", update_root_string);

			let _ = generate_stage_1_proof(&client, &relay_client).await;

			if onchain_root == root {
				log::info!("onchain_root matches!!! submitting now!!");
				let submit_proof_string = match submit_proof(&client, proof.clone()).await {
					Ok(_) => {
						root = generate_mmr_root(&client, Some(0)).await?;
						proof = generate_mmr_proof(&client, 1u64, Some(0)).await?;
						"Submit proof successfully submitted".to_string()
					},
					Err(e) => format!("Cant submit proof on chain yet {:?}", e),
				};
				log::info!("{}", submit_proof_string);
			}
			else {
				log::info!("Root on chain {:?} still doesnt match original root {:?}", onchain_root, root);
			}
		}
		log::info!("EXITING!!!!!");
		Ok::<(), anyhow::Error>(())
	});
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

				log::debug!("Block for {:?}: is block_hash {:?}, block_number {:?}",
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

async fn try_generate_mmr_proof(
	client: &MultiClient,
	block_num: u64,
	channel_id: Option<u64>
) -> anyhow::Result<LeavesProof<H256>> {
	let client = client.clone();
	const MAX_ATTEMPTS: u32 = 3;
    const RETRY_INTERVAL: u64 = 4;
	let mut attempts = 0u32;

	loop {
		log::info!("Entering generate_mmr_proof block_num {}", block_num);

		let params = match channel_id {
            Some(id) => rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None, id],
            None => rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None],
        };

		match client.rpc_client.request::<Option<LeavesProof<H256>>, _>("mmr_generateProof", params).await {
            Ok(opt_proof) => {
                log::info!("MMR proof obtained for block_num {}", block_num);
                return opt_proof.ok_or(anyhow::anyhow!("Proof was None"))
            },
            Err(e) if attempts < MAX_ATTEMPTS => {
                log::error!("Failed to obtain MMR proof for block_num {}: {:?}", block_num, e);
                attempts += 1;
                tokio::time::sleep(Duration::from_secs(RETRY_INTERVAL)).await;
				continue;
            },
            Err(e) => return Err(e.into()),
        };
	}
}

// Call generate mmr proof for sender
async fn generate_mmr_proof(
	client: &MultiClient,
	block_num: u64,
	channel_id: Option<u64>
) -> anyhow::Result<LeavesProof<H256>> {
	log::info!("Entering generate_mmr_proof block_num {}", block_num);
	let block = client.subxt_client.blocks().at_latest().await?;

	let mut params = jsonrpsee::core::params::ArrayParams::new();
	if let Some(id) = channel_id {
		params = rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None, id];
	}
	else {
		params = rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None];
	}


	let request: Option<LeavesProof<H256>> = match client.rpc_client.request("mmr_generateProof", params).await {
		Ok(opt_proof) => {
			log::info!("Got proof from request.. block_num {}", block_num);
			opt_proof
		},
		Err(e) => {
			log::info!("Couldnt Get MMR request error {:?}", e);
			None
		}
	};
	// let request: Option<LeavesProof<H256>> = client.rpc_client.request("mmr_generateProof", params).await?;
	let proof = request.ok_or(RelayerError::Default)?;
	log::info!("Proof obtained:: {:?}", proof);
	Ok(proof)
}

// Call generate mmr proof for sender
async fn generate_mmr_root(client: &MultiClient, channel_id: Option<u64>) -> anyhow::Result<H256> {
	let block = client.subxt_client.blocks().at_latest().await?;


	let mut params = jsonrpsee::core::params::ArrayParams::new();
	if let Some(id) = channel_id {
		params = rpc_params![Option::<Hash>::None, id];
	}
	else {
		params = rpc_params![Option::<Hash>::None];
	}

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