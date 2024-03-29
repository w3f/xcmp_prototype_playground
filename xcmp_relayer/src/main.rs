use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use parity_scale_codec::{Encode, Decode};
use cumulus_primitives_core::xcmr_digest::extract_xcmp_channel_merkle_root;
use runtime::{pallet_xcmp_message_stuffer::ChannelMerkleProof, BlockNumber, Hash, Header};
use sp_mmr_primitives::EncodableOpaqueLeaf;
use sp_consensus_beefy::mmr::{MmrLeafVersion, MmrLeaf};
use std::{
	collections::BTreeMap, ops::Sub, sync::Mutex, time::Duration
};

use lazy_static::lazy_static;

use sp_core::H256;

use tokio::task;

use subxt::{OnlineClient, PolkadotConfig};

use mmr_rpc::LeavesProof;

use subxt_signer::sr25519::dev;

#[subxt::subxt(runtime_metadata_url = "ws://localhost:54887")]
pub mod polkadot { }

#[subxt::subxt(runtime_metadata_url = "ws://localhost:54886")]
pub mod relay { }

use relay::runtime_types::polkadot_runtime_parachains::paras::{
	ParaMerkleProof as RelayParaMerkleProof, ParaLeaf as RelayParaLeaf
};

use relay::runtime_types::polkadot_parachain_primitives::primitives::Id as ParaId;

use polkadot::runtime_types::{
	pallet_xcmp_message_stuffer::XcmpProof,
	sp_mmr_primitives::Proof as XcmpProofType,
	polkadot_runtime_parachains::paras::{ParaMerkleProof, ParaLeaf},
	polkadot_parachain_primitives::primitives::Id as ParachainParaId,
	pallet_xcmp_message_stuffer::ChannelMerkleProof as SubxtChannelMerkleProof,
};

/// The default endpoints for each
const DEFAULT_ENDPOINT_PARA_SENDER: &str = "ws://localhost:54888";
const DEFAULT_RPC_ENDPOINT_PARA_SENDER: &str = "http://localhost:54888";
const DEFAULT_ENDPOINT_PARA_RECEIVER: &str = "ws://localhost:54887";
const DEFAULT_RPC_ENDPOINT_PARA_RECEIVER: &str = "http://localhost:54887";
const DEFAULT_ENDPOINT_RELAY: &str = "ws://localhost:54886";
const DEFAULT_RPC_ENDPOINT_RELAY: &str = "http://localhost:54886";

const RECEIVER_CHANNEL_ID: u64 = 0;

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

impl From<SubxtChannelMerkleProof> for ChannelMerkleProof {
	fn from(x: SubxtChannelMerkleProof) -> Self {
		Self {
			root: x.root,
			proof: x.proof,
			num_leaves: x.num_leaves,
			leaf_index: x.leaf_index,
			leaf: x.leaf
		}
	}
}

impl From<ChannelMerkleProof> for SubxtChannelMerkleProof {
	fn from(x: ChannelMerkleProof) -> Self {
		Self {
			root: x.root,
			proof: x.proof,
			num_leaves: x.num_leaves,
			leaf_index: x.leaf_index,
			leaf: x.leaf
		}
	}
}

impl From<RelayParaMerkleProof> for ParaMerkleProof {
	fn from(x: RelayParaMerkleProof) -> Self {
		Self {
			root: x.root,
			proof: x.proof,
			num_leaves: x.num_leaves,
			leaf_index: x.leaf_index,
			leaf: x.leaf.into(),
		}
	}
}

impl From<RelayParaLeaf> for ParaLeaf {
	fn from(x: RelayParaLeaf) -> Self {
		Self {
			para_id: x.para_id.into(),
			head_data: x.head_data,
		}
	}
}

impl From<ParaId> for ParachainParaId {
	fn from(x: ParaId) -> Self {
		Self(x.0)
	}
}

impl Clone for RelayParaMerkleProof {
	fn clone(&self) -> Self {
		Self {
			root: self.root.clone(),
			proof: self.proof.clone(),
			num_leaves: self.num_leaves.clone(),
			leaf_index: self.leaf_index.clone(),
			leaf: self.leaf.clone(),
		}
	}
}

impl Clone for SubxtChannelMerkleProof {
	fn clone(&self) -> Self {
		Self {
			root: self.root.clone(),
			proof: self.proof.clone(),
			num_leaves: self.num_leaves.clone(),
			leaf_index: self.leaf_index.clone(),
			leaf: self.leaf.clone(),
		}
	}
}

impl Clone for ParaId {
	fn clone(&self) -> Self {
		Self(self.0)
	}
}

impl Clone for RelayParaLeaf {
	fn clone(&self) -> Self {
		Self {
			para_id: self.para_id.clone().into(),
			head_data: self.head_data.clone()
		}
	}
}

type BeefyMmrRoot = H256;
type RelayBlockIndex = u32;

lazy_static! {
	// Mapping (ParaHeaderRoot, RelayBlockNum) -> (BeefyMmrRoot, Proof)
	static ref LEAVES_BEEFY_MMR_MAP: Mutex<BTreeMap<(H256, RelayBlockIndex), (BeefyMmrRoot, LeavesProof<H256>)>> = Mutex::new(BTreeMap::new());
	static ref PARA_HEADERS_MAP: Mutex<BTreeMap<BeefyMmrRoot, RelayParaMerkleProof>> = Mutex::new(BTreeMap::new());
	static ref MSG_ROOT_MAP: Mutex<BTreeMap<H256, LeavesProof<H256>>> = Mutex::new(BTreeMap::new());
	static ref MSG_ROOT_CHANNEL_MERKLE_MAP: Mutex<BTreeMap<H256, ChannelMerkleProof>> = Mutex::new(BTreeMap::new());
	static ref CHANNEL_ROOT_PROOF_MAP: Mutex<BTreeMap<H256, ChannelMerkleProof>> = Mutex::new(BTreeMap::new());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	log::info!("Booting up");

	let para_sender_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_SENDER, DEFAULT_RPC_ENDPOINT_PARA_SENDER ).await?;
	let para_receiver_api = MultiClient::new(DEFAULT_ENDPOINT_PARA_RECEIVER, DEFAULT_RPC_ENDPOINT_PARA_RECEIVER).await?;
	let relay_api = MultiClient::new(DEFAULT_ENDPOINT_RELAY, DEFAULT_RPC_ENDPOINT_RELAY).await?;

	let _ = collect_relay_data(&relay_api).await?;
	let _ = collect_para_data(&para_sender_api).await?;

	let _ = log_all_mmr_roots(&para_sender_api).await?;
	let _ = log_all_mmr_proofs(&para_sender_api).await?;

	let _ = get_proof_and_verify(&para_sender_api, &relay_api, &para_receiver_api).await?;

	let _subscribe = log_all_blocks(&vec![para_sender_api, para_receiver_api, relay_api]).await?;

	std::future::pending::<()>().await;
	Ok(())
}

async fn collect_relay_data(client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		while let Some(block) = blocks_sub.next().await {
			let block = block?;

			let params = rpc_params![Option::<Hash>::None, 0u64];
			log::debug!("Before relay beefy root request");
			let request: Option<Hash> = match client.rpc_client.request("mmr_root", params).await {
				Ok(opt) => {
					log::debug!("Relay MMR root request success");
					opt
				},
				Err(e) => {
					log::error!("Relay MMR root request failed with {:?}", e);
					None
				}
			};

			log::debug!("After relay beefy root request");
			let root = request.ok_or(RelayerError::Default)?;
			log::debug!("Got Current Beefy Root from Relaychain {:?}", root);

			let request: Option<Header> = match client.rpc_client.request("chain_getHeader", rpc_params![Option::<Hash>::None]).await {
				Ok(block) => {
					block
				},
				Err(e) => {
					log::error!("Could not get block from chain with error {:?}", e);
					None
				}
			};
			log::debug!("After chain block request");
			let rpc_header = request.ok_or(RelayerError::Default)?;
			if rpc_header.number != block.number() {
				log::error!(
					"RPC_Requested Block does not match Loop Block {} != {}",
					rpc_header.number,
					block.number()
				);
			}

			let proof = generate_mmr_proof(&client, block.number().into(), Some(RECEIVER_CHANNEL_ID), Some(block.hash()))
				.await
				.map_err(|e| {
					log::error!("Failed to generate big proof from Relaychain with error {:?}", e);
					RelayerError::Default
				}
			)?;

			// Decode Beefy proof leaves into MmrLeaf type get ParaHeaderRoot from extra_data field
			// First go from EncodableOpaqueLeaf -> OpaqueLeaf -> MmrLeaf
			let beefy_leaves: Vec<EncodableOpaqueLeaf> = Decode::decode(&mut &proof.leaves.0[..])
			.map_err(|e| anyhow::Error::new(e))?;

			// Search through each leaf for the para_header_root from extra_data field
			// the element should be one leaf since we only request leaves proof for one blocknum
			if beefy_leaves.len() > 1 {
				log::error!("Beefy leaves are greater than 1!!!!! issue storing");
			}

			let leaf = &beefy_leaves[0];
			let leaf = leaf.clone().into_opaque_leaf();

			let mmr_leaf: MmrLeaf<BlockNumber, Hash, H256, H256> = Decode::decode(&mut &leaf.0[..])
				.map_err(|e| {
					log::error!("COULD NOT DECODE LEAF!! WITH ERROR {:?}", e);
					anyhow::Error::new(e)
			})?;

			// Store as (ParaHeaderRoot, RelayBlockNum) -> (BeefyMmrRoot, Proof)
			match LEAVES_BEEFY_MMR_MAP.try_lock() {
				Ok(mut s) => {
					log::debug!(
						"Inserting into storage LEAVES_BEEFY_MMR_MAP for MMR_LEAF {:?}",
						mmr_leaf
					);
					s.insert((mmr_leaf.leaf_extra, block.number()), (root.clone(), proof.clone()));
				},
				Err(_) => log::error!("Could not lock LEAVES_BEEFY_MMR_MAP for writing")
			}

			let id = ParaId(1000u32);

			let para_merkle_call = relay::apis().paras_api().get_para_heads_proof(id);
			let para_merkle_proof =  match client.subxt_client
				.runtime_api()
				.at(block.reference())
				.call(para_merkle_call)
			.await {
				Ok(opt_proof) => {
					opt_proof
				},
				Err(e) => {
					log::error!("Error calling Para Merkle Proof Api with error {:?}", e);
					None
				}
			};

			let para_merkle_proof = para_merkle_proof.ok_or_else(|| {
				log::error!("No Para Merkle Proof obtained!!!!");
				RelayerError::Default
			})?;

			let para_header_root  = match extract_xcmp_channel_root(para_merkle_proof.leaf.clone().into()).await {
				Ok(root) => {
					log::debug!("Can decode extract root from Header Digest XcmpChannelRoot is {:?}", root);
					root
				},
				Err(_) => {
					log::error!("COULD not extract Digest IN EXTRACT function!!!");
					H256::zero()
				}
			};

			if para_header_root == H256::zero() {
				log::error!("ParaHeader root not extracted zero H256 value");
				return Err(RelayerError::Default.into())
			}

			// Now have the `para_header_root` which will be a leaf in the Beefy Mmr
			match PARA_HEADERS_MAP.try_lock() {
				Ok(mut s) => {
					log::debug!(
						"Inserting into storage PARA_MERKLE_PROOF for relay block number {}, para_merkle_proof {:?}",
						block.number(),
						para_merkle_proof.clone()
					);
					s.insert(para_header_root, para_merkle_proof.clone());
				},
				Err(_) => log::error!("Could not lock PARA_HEADERS_MAP for writing")
			}

			log::debug!("Beefy Mmr Root from Relaychain Obtained:: {:?}", root);
			log::debug!("Para Merkle Proof from Relaychain Obtained:: {:?}", para_merkle_proof);
		}
		Ok::<(), anyhow::Error>(())
	});
	Ok(())
}



async fn extract_xcmp_channel_root(leaf: ParaLeaf) -> anyhow::Result<H256> {
	// First decode ParaLeaf.head_data into a ParaHeader
	let header = Header::decode(&mut &leaf.head_data[..])?;

	log::debug!("EXTRACTED AND DECODED HEADERRR DANKEEEE!!!!!!{:?}", header);

	// extracting root from digest
	let xcmp_channel_root: H256 = extract_xcmp_channel_merkle_root(&header.digest).ok_or(RelayerError::Default)?;

	// Extract the XcmpChannelBinaryMerkleRoot from the Digest
	Ok(xcmp_channel_root)
}

#[tokio::test]
async fn test_decode() {
	let header = Header {
		parent_hash: H256::zero(),
		state_root: H256::zero(),
		extrinsics_root: H256::zero(),
		number: 0u32,
		digest: Default::default(),
	};
	let dummy_input = ParaLeaf { para_id: ParaId(1000).into(), head_data: header.encode() };
	let result = extract_xcmp_channel_root(dummy_input).await;
	assert!(result.is_ok());
	assert_eq!(result.unwrap(), H256::zero());
}

async fn collect_para_data(client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	task::spawn(async move {
		let mut blocks_sub = client.subxt_client.blocks().subscribe_best().await?;
		while let Some(block) = blocks_sub.next().await {
			let block = block?;

			let msg_root = generate_mmr_root(&client, Some(RECEIVER_CHANNEL_ID), Some(block.hash())).await?;
			let msg_proof = generate_mmr_proof(&client, block.number().into(), Some(RECEIVER_CHANNEL_ID), Some(block.hash())).await?;

			match MSG_ROOT_MAP.try_lock() {
				Ok(mut s) => {
					s.insert(msg_root.clone(), msg_proof);
					log::debug!(
						"Inserting into storage Msg root {:?}",
						msg_root
					);
				},
				Err(_) => log::error!("Could not lock MSG_ROOT_MAP for writing")
			}

			let channels_merkle_call = polkadot::apis().channel_merkle_api().get_xcmp_channels_proof(RECEIVER_CHANNEL_ID);
			let channel_merkle_proof = match client.subxt_client
				.runtime_api()
				.at(block.reference())
				.call(channels_merkle_call)
				.await {
					Ok(proof) => {
						log::debug!("Got channels merkle proof from 'get_xcmp_channels_proof' {:?}", proof);
						proof
					},
					Err(e) => {
						log::debug!("Got error from trying to call `get_xcmp_channels_proof` {:?}", e);
						None
					}
				}.ok_or_else(|| {
					log::error!("error in generating stage 3 of proof exited generate_xcmp_proof");
					RelayerError::Default
				})?;

			// For grabbing the channel merkle proofs by their respective root later
			match CHANNEL_ROOT_PROOF_MAP.try_lock() {
				Ok(mut s) => {
					s.insert(channel_merkle_proof.root, channel_merkle_proof.clone().into());
				},
				Err(_) => {
					log::error!("Could not lock Channel ROOT Proof MAP for writing")
				}
			}

			let roots_match = match MSG_ROOT_MAP.try_lock() {
				Ok(mut s) => {
					if s.contains_key(&channel_merkle_proof.leaf.clone()) {
						true
					}
					else {
						false
					}
				}
				Err(_) => {
					log::error!("Could not lock MSG_ROOT_MAP for writing");
					false
				}
			};

			if !roots_match {
				log::error!("Roots dont match between msg_mmr_root and channel_merkle_root!!@£!@£");
				continue;
			}

			match MSG_ROOT_CHANNEL_MERKLE_MAP.try_lock() {
				Ok(mut s) => {
					s.insert(msg_root.clone(), channel_merkle_proof.into());
					log::debug!(
						"Inserting into storage msg_root for channel_merkle_proof {:?}",
						msg_root
					);
				},
				Err(_) => log::error!("Could not lock MSG_ROOT_CHANNEL_MERKLE_MAP for writing")
			}

		}
		Ok::<(), anyhow::Error>(())
	});
	Ok(())
}

// TODO: Add documentation for this function and add helper functions for each step to make it smaller and
// easier to digest
async fn generate_xcmp_proof(client: &MultiClient, _relay_client: &MultiClient, recv_client: &MultiClient) -> anyhow::Result<()> {
	log::info!("Entered generate xcmp proof");
	let client = client.clone();
	let recv_client = recv_client.clone();

	let block = client.subxt_client.blocks().at_latest().await?;

	let (stage_4_root, stage_4_proof) = match MSG_ROOT_MAP.try_lock() {
		Ok(s) => {
			s.iter().next().map(|(key, value)| (key.clone(), value.clone()))
		},
		Err(_) => {
			log::error!("Could not lock MSG_ROOT_MAP for reading");
			None
		}
	}.ok_or_else(|| {
			log::error!("Could not read stage_4_root and stage_4_proof from MSG_ROOT_MAP");
			RelayerError::Default
		}
	)?;

	let roots_match = match MSG_ROOT_CHANNEL_MERKLE_MAP.try_lock() {
		Ok(s) => {
			if s.contains_key(&stage_4_root) {
				true
			}
			else {
				false
			}
		}
		Err(_) => {
			log::error!("Could not lock MSG_ROOT_CHANNEL_MERKLE_MAP for writing");
			false
		}
	};

	if !roots_match {
		log::error!("Stage 4 root doesnt exist yet for stage 3 generation proof generation failed..");
		match MSG_ROOT_CHANNEL_MERKLE_MAP.lock() {
			Ok(s) => {
				log::error!("Couldnt find stage_4_root: {:?}, in MSG_ROOT_CHANNEL_MERKLE_MAP {:?}", stage_4_root, s);
			},
			Err(e) => {
				log::error!("Could not lock MSG_ROOT_CHANNEL_MERKLE_MAP for PRINTING")
			}
		}
		return Err(RelayerError::Default.into())
	}

	let stage_4_leaves = Decode::decode(&mut &stage_4_proof.leaves.0[..])
			.map_err(|e| anyhow::Error::new(e))?;
	let stage_4_decoded_proof: XcmpProofType<H256> = Decode::decode(&mut &stage_4_proof.proof.0[..])
			.map_err(|e| anyhow::Error::new(e))?;


	let stage_3_proof = match MSG_ROOT_CHANNEL_MERKLE_MAP.try_lock() {
		Ok(s) => {
			s.get(&stage_4_root).cloned()
		},
		Err(_) => {
			log::error!("Could not lock MSG_ROOT_CHANNEL_MERKLE_MAP for reading");
			None
		}
	}.ok_or_else(|| {
			log::error!("Could not read stage_3_root and stage_3_proof from MSG_ROOT_CHANNEL_MERKLE_MAP");
			RelayerError::Default
		}
	)?;

	let stage_2_proof = match PARA_HEADERS_MAP.try_lock() {
		Ok(s) => {
			s.get(&stage_3_proof.root.clone()).cloned()
		},
		Err(_) => {
			log::error!("Could not lock PARA_HEADERS_MAP for reading");
			None
		},
	}.ok_or_else(|| {
		log::error!("Could not read stage_2_root and stage_2 proof from PARA_HEADERS_MAP for stage 3 root {:?}", stage_3_proof.leaf);
		RelayerError::Default
	})?;

	// Lookup in `LEAVES_BEEFY_MMR_MAP` all values for keys (state_2_proof.root, _) collect into a vec
	// Log size of this
	// If size is 1 then place the proof and root into the stage_1 proof if not log error and take last entry
	let beefy_entries = match LEAVES_BEEFY_MMR_MAP.try_lock() {
		Ok(mut s) => {
			s.iter()
				.filter(|((root, _), _)| *root == stage_2_proof.root)
				.map(|(_, value)| value.clone())
				.collect::<Vec<_>>()
		},
		Err(_) => {
			log::error!("Could not lock PARA_HEADERS_MAP for reading");
			Vec::new()
		}
	};

	if beefy_entries.is_empty() {
		log::error!("Beefy entries vec is empty.. issue with LOCK or lookup!");
		return Err(RelayerError::Default.into())
	}

	if beefy_entries.len() > 1 {
		log::error!("Multiple Beefy Entries for same para_header_root taking the last!!!");
	}

	let stage_1_beefy_root = &beefy_entries[beefy_entries.len() - 1].0;
	let stage_1_proof = &beefy_entries[beefy_entries.len() - 1].1;

	let stage_1_leaves = Decode::decode(&mut &stage_1_proof.leaves.0[..])
			.map_err(|e| anyhow::Error::new(e))?;
	let stage_1_decoded_proof: XcmpProofType<H256> = Decode::decode(&mut &stage_1_proof.proof.0[..])
			.map_err(|e| anyhow::Error::new(e))?;

	match MSG_ROOT_MAP.try_lock() {
		Ok(mut s) => {
			s.remove(&stage_4_root)
		},
		Err(_) => {
			log::error!("Could not lock MSG_ROOT_MAP for reading");
			None
		}
	}.ok_or_else(|| {
			log::error!("Could not read stage_4_root and stage_4_proof from MSG_ROOT_MAP");
			RelayerError::Default
		}
	)?;

	// Checking which is the current beefy root on chain for logging
	let beefy_api_call = polkadot::apis().messaging_api().get_current_beefy_root();
	let beefy_root = match client.subxt_client
		.runtime_api()
		.at_latest()
		.await?
		.call(beefy_api_call)
		.await {
			Ok(root) => {
				log::debug!("Got root from `get_current_beefy_root` {:?}", root);
				root
			},
			Err(e) => {
				log::error!("Got error from trying to call `get_current_beefy_root` {:?}", e);
				H256::zero()
			}
		};

	log::info!("Beefy Root on Parachain {:?} Beefy Root needed to verify proof is {:?}", beefy_root, stage_1_beefy_root);

	let xcmp_proof = XcmpProof {
		stage_1: (stage_1_decoded_proof, stage_1_leaves),
		stage_2: stage_2_proof.into(),
		stage_3: stage_3_proof.into(),
		stage_4: (stage_4_decoded_proof, stage_4_leaves),
	};

	// 3.) Send transaction to chain for proof
	log::info!("calling submit_xcmp_proof");
	submit_xcmp_proof(&client, xcmp_proof, *stage_1_beefy_root).await?;

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
				Ok(events) => { log::debug!("Got the tx in a block and it succeeded! {:?}", events); },
				Err(e) => { log::error!("Was not successful extrinsic ERROR:: {:?}", e); }
			}
		},
		Err(e) => {
			log::debug!("Tx didnt get in a block error {:?}", e);
		}
	}
	log::debug!("Hash of update_root: {:?}", hash_tx);
	Ok(())
}

async fn submit_xcmp_proof(client: &MultiClient, xcmp_proof: XcmpProof, beefy_root: H256) -> anyhow::Result<()> {
	log::info!("Entering `submit_xcmp_proof`");
	let signer = dev::charlie();

	let tx = crate::polkadot::tx().msg_stuffer_para_a().submit_xcmp_proof(xcmp_proof, beefy_root);
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
	log::info!("Hash of XCMP_proof_submission: {:?}", hash_tx);

	Ok(())
}

async fn submit_proof(client: &MultiClient, proof: LeavesProof<H256>) -> anyhow::Result<()> {
	let signer = dev::bob();
	let channel_id = 0u64;
	let leaves = Decode::decode(&mut &proof.leaves.0[..])
					.map_err(|e| anyhow::Error::new(e))?;

	log::debug!("Decoded leaves {:?}", leaves);

	let decoded_proof = Decode::decode(&mut &proof.proof.0[..])
			.map_err(|e| anyhow::Error::new(e))?;

	log::debug!("Decoded proof {:?}", decoded_proof);

	let tx = crate::polkadot::tx().msg_stuffer_para_a().submit_test_proof(decoded_proof, leaves, channel_id);

	let tx_progress = client.subxt_client.tx().sign_and_submit_then_watch_default(&tx, &signer).await?;
	let hash_tx = tx_progress.extrinsic_hash();
	log::debug!("Got After submitting submit_test_proof");
	match tx_progress.wait_for_in_block().await {
		Ok(tx_in_block) => {
			match tx_in_block.wait_for_success().await {
				Ok(events) => { log::debug!("Got the tx in a block and it succeeded! {:?}", events); },
				Err(e) => { log::debug!("Was not successful extrinsic ERROR:: {:?}", e); }
			}
		},
		Err(e) => {
			log::debug!("Tx didnt get in a block error {:?}", e);
		}
	}
	log::debug!("Hash of test_proof_submission: {:?}", hash_tx);

	Ok(())
}

async fn get_proof_and_verify(client: &MultiClient, relay_client: &MultiClient, recv_client: &MultiClient) -> anyhow::Result<()> {
	let client = client.clone();
	let relay_client = relay_client.clone();
	let recv_client = recv_client.clone();
	let mut root = generate_mmr_root(&client, Some(RECEIVER_CHANNEL_ID), None).await?;
	let mut proof = generate_mmr_proof(&client, 1u64, Some(RECEIVER_CHANNEL_ID), None).await?;

	let params = rpc_params![root, proof.clone()];

	let request: Option<bool> = client.rpc_client.request("mmr_verifyProofStateless", params).await?;
	let verification = request.ok_or(RelayerError::Default)?;
	log::debug!("Was proof verified? Answer:: {}", verification);

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
				Err(_) => H256::zero()
			};

			let update_root_string = match update_root(&client, root).await {
				Ok(_) => {
					"Updating root in loop".to_string()
				},
				Err(e) => format!("Cant update root on chain yet {:?}", e),
			};
			log::debug!("{}", update_root_string);

			let _ = generate_xcmp_proof(&client, &relay_client, &recv_client).await;

			if onchain_root == root {
				log::debug!("onchain_root matches!!! submitting now!!");
				let submit_proof_string = match submit_proof(&client, proof.clone()).await {
					Ok(_) => {
						root = generate_mmr_root(&client, Some(RECEIVER_CHANNEL_ID), None).await?;
						proof = generate_mmr_proof(&client, 1u64, Some(RECEIVER_CHANNEL_ID), None).await?;
						"Submit proof successfully submitted".to_string()
					},
					Err(e) => format!("Cant submit proof on chain yet {:?}", e),
				};
				log::debug!("{}", submit_proof_string);
			}
			else {
				log::debug!("Root on chain {:?} still doesnt match original root {:?}", onchain_root, root);
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
			log::debug!("Mmr root for block_hash {:?}, block_number {:?} root {:?}", block.hash(), block.number(), hash);
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
			log::debug!("num of mmr blocks: {}, counter value: {}, proof_size: {}, curr_avg_proof_size: {}", mmr_blocks_to_query.len(), mmr_blocks_benchmark_counter, curr_proof_size, mean);
			log::debug!("Mmr proof for block_hash {:?}, block_number {:?}, proof {:?}", block.hash(), block.number(), proof);
		}

		Ok::<(), anyhow::Error>(())
	});

	Ok(())
}

async fn do_mean(vec: &[u64]) -> anyhow::Result<u64> {
	let mut sum = 0u64;
	let len = vec.len() as u64;
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
		log::debug!("Entering generate_mmr_proof block_num {}", block_num);

		let params = match channel_id {
            Some(id) => rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None, id],
            None => rpc_params![vec![block_num], Option::<BlockNumber>::None, Option::<Hash>::None],
        };

		match client.rpc_client.request::<Option<LeavesProof<H256>>, _>("mmr_generateProof", params).await {
            Ok(opt_proof) => {
                log::debug!("MMR proof obtained for block_num {}", block_num);
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
	channel_id: Option<u64>,
	block_hash: Option<Hash>
) -> anyhow::Result<LeavesProof<H256>> {
	log::debug!("Entering generate_mmr_proof block_num {}", block_num);

	let params = if let Some(id) = channel_id {
		rpc_params![vec![block_num], Option::<BlockNumber>::None, block_hash, id]
	}
	else {
		rpc_params![vec![block_num], Option::<BlockNumber>::None, block_hash]
	};

	let request: Option<LeavesProof<H256>> = match client.rpc_client.request("mmr_generateProof", params).await {
		Ok(opt_proof) => {
			log::debug!("Got proof from request.. block_num {}", block_num);
			opt_proof
		},
		Err(e) => {
			log::error!("Couldnt Get MMR request error {:?}", e);
			None
		}
	};
	let proof = request.ok_or(RelayerError::Default)?;
	log::debug!("Proof obtained:: {:?}", proof);
	Ok(proof)
}

// Call generate mmr proof for sender
async fn generate_mmr_root(client: &MultiClient, channel_id: Option<u64>, block_hash: Option<Hash>) -> anyhow::Result<H256> {
	let params = if let Some(id) = channel_id {
		rpc_params![block_hash, id]
	}
	else {
		rpc_params![block_hash]
	};

	let request: Option<Hash> = client.rpc_client.request("mmr_root", params).await?;
	let root = request.ok_or(RelayerError::Default)?;
	log::debug!("root obtained:: {:?}", root);
	Ok(root)
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