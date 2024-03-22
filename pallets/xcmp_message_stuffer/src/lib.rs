#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash, verify_leaves_proof};

use frame_support::{dispatch::{DispatchResult}, pallet_prelude::*, WeakBoundedVec};
use frame_system::pallet_prelude::*;
use cumulus_primitives_core::{ParaId, GetBeefyRoot, xcmr_digest::extract_xcmp_channel_merkle_root};
use sp_runtime::traits::{Hash as HashT, Keccak256};
use sp_core::H256;
use polkadot_runtime_parachains::paras::{ParaMerkleProof, ParaLeaf};
use binary_merkle_tree::Leaf;

use sp_mmr_primitives::{Proof, EncodableOpaqueLeaf, DataOrHash};
use sp_consensus_beefy::mmr::{MmrLeafVersion, MmrLeaf as BeefyMmrLeaf};
use scale_info::prelude::vec::Vec;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

const LOG_TARGET: &str = "runtime::xmp_message_stuffer";

pub trait XcmpMessageProvider<Hash> {
	type XcmpMessages: Encode + Decode + scale_info::prelude::fmt::Debug + PartialEq + Clone + TypeInfo;

	fn get_xcmp_messages(block_hash: Hash, para_id: ParaId) -> Self::XcmpMessages;
}

#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct ChannelMerkleProof {
	pub root: H256,
	pub proof: Vec<H256>,
	pub num_leaves: u64,
	pub leaf_index: u64,
	pub leaf: H256,
}

impl Default for ChannelMerkleProof {
	fn default() -> Self {
		ChannelMerkleProof {
			root: H256::zero(),
			proof: Vec::new(),
			num_leaves: 0u64,
			leaf_index: 0u64,
			leaf: H256::zero(),
		}
	}
}

type XcmpMessages<T, I> = <<T as crate::Config<I>>::XcmpDataProvider as XcmpMessageProvider<<T as frame_system::Config>::Hash>>::XcmpMessages;
type MmrProof = Proof<H256>;
type ChannelId = u64;

#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct XcmpProof {
	// TODO: Probably should rename each of these stages to some fancy name
	// TODO: Remove tuples
	pub stage_1: (MmrProof, Vec<EncodableOpaqueLeaf>),
	pub stage_2: ParaMerkleProof,
	pub stage_3: ChannelMerkleProof,
	pub stage_4: (MmrProof, Vec<EncodableOpaqueLeaf>),
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	#[pallet::config]
	pub trait Config<I: 'static = ()>: frame_system::Config + pallet_mmr::Config<I> {
		type ParaIdentifier: Get<ParaId>;
		type RuntimeEvent: From<Event<Self, I>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type LeafVersion: Get<MmrLeafVersion>;
		type XcmpDataProvider: XcmpMessageProvider<Self::Hash>;
		type RelayerOrigin: EnsureOrigin<Self::RuntimeOrigin>;
		/// This is used when updating the current `XcmpChannelRoots`
		type BeefyRootProvider: GetBeefyRoot;
		type MaxBeefyRootsKept: Get<u32>;
	}

	#[pallet::pallet]
	pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

	/// These are the MMR roots for each open XCMP channel as verified against current Relay Chain Beefy Root
	#[pallet::storage]
	#[pallet::getter(fn xcmp_channel_roots)]
	pub type XcmpChannelRoots<T: Config<I>, I: 'static = ()> = StorageMap<_, Identity, ChannelId, H256, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn seen_beefy_roots)]
	pub type SeenBeefyRoots<T: Config<I>, I: 'static = ()> = StorageMap<_, Identity, H256, BlockNumberFor<T>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn seen_beefy_roots_order)]
	pub type SeenBeefyRootsOrder<T: Config<I>, I: 'static = ()> = StorageValue<_, WeakBoundedVec<H256, T::MaxBeefyRootsKept>, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config<I>, I: 'static = ()> {
		XcmpMessageSent {
			msg_hash: T::Hash,
			block_num: BlockNumberFor<T>,
		},
		XcmpMessagesAccepted {
			mmr_channel_root: H256,
			channel_id: ChannelId,
		},
	}

	#[pallet::error]
	pub enum Error<T, I = ()> {
		XcmpProofNotValid,
		XcmpProofAccepted,
		XcmpProofLeavesNotValid,
		XcmpNoChannelRootForChannelId,
		XcmpBeefyRootTargetedNeverSeen,
		XcmpStage1LeavesTooLarge,
		XcmpStage1ProofDoesntVerify,
		XcmpStage2RootDoesntMatch,
		XcmpStage2LeafDoesntDecode,
		XcmpStage2ProofDoesntVerify,
		XcmpStage3HeaderDoesntDecode,
		XcmpStage3CannotBeExtracted,
		XcmpStage3RootDoesntMatch,
		XcmpStage3ProofDoesntVerify,
		XcmpStage4ProofDoesntVerify,
	}

	#[pallet::hooks]
	impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {
		fn on_initialize(n: BlockNumberFor<T>) -> Weight {
			// TODO: Remove Temporary.. change from unwrapping default..
			let beefy_root = T::BeefyRootProvider::get_root().unwrap_or_default();
			SeenBeefyRoots::<T, I>::insert(&beefy_root.clone().into(), n);

			let mut order = SeenBeefyRootsOrder::<T, I>::get().into_inner();
			order.push(beefy_root.into());

			let item = WeakBoundedVec::force_from(order, None);
			SeenBeefyRootsOrder::<T, I>::put(item);

			T::DbWeight::get().writes(3)
		}
	}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {

		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn submit_test_proof(origin: OriginFor<T>, mmr_proof: MmrProof, leaves: Vec<EncodableOpaqueLeaf>, channel_id: u64) -> DispatchResult {
			ensure_signed(origin)?;

			log::info!(
				target: LOG_TARGET,
				"Called submit xcmp proof",
			);

			let nodes: Vec<_> = leaves
				.clone()
				.into_iter()
				.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
				.collect();

			let root = Self::xcmp_channel_roots(channel_id).ok_or(Error::<T, I>::XcmpNoChannelRootForChannelId)?;

			log::info!(
				target: LOG_TARGET,
				"obtained xcmp channel root {:?}",
				root
			);

			verify_leaves_proof(root, nodes, mmr_proof).map_err(|_| Error::<T, I>::XcmpProofNotValid)?;

			log::info!(
				target: LOG_TARGET,
				"Verified XCMP Channel Mmr proof",
			);

			Self::deposit_event(Event::XcmpMessagesAccepted { mmr_channel_root: root, channel_id: channel_id } );

			// TODO:
			// 1.) Get latest XcmpChannelRoot using 'channel_id'
			// 2.) Verify MmrProof by calling verify with MmrChannelRoot and MmrProof
			// 3.) if passes check then start process of decoding the XCMP blob into its XCM components
			// 4.) Process XCM messages

			Ok(())
		}

		/// TODO: This is just for testing relayer for now. The root should be updated by checking
		/// the relaychain updated XCMPTrie remove
		#[pallet::call_index(1)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn update_root(origin: OriginFor<T>, root: H256, channel_id: u64) -> DispatchResult {
			ensure_signed(origin)?;
			XcmpChannelRoots::<T, I>::insert(&channel_id, root);
			log::info!(
				target: LOG_TARGET,
				"Updated root for channel_id: {:?}, root: {:?}",
				channel_id, root
			);
			Ok(())
		}

		/// For now there is just one leaf in each membership proof
		/// TODO: Change to support multiple leaves..
		#[pallet::call_index(2)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn submit_xcmp_proof(origin: OriginFor<T>, xcmp_proof: XcmpProof, beefy_root_targeted: H256) -> DispatchResult {
			ensure_signed(origin)?;

			log::info!(
				target: LOG_TARGET,
				"Called submit big proof, targeting BEEFY_ROOT {:?}",
				beefy_root_targeted
			);

			if !SeenBeefyRoots::<T, I>::contains_key(&beefy_root_targeted) {
				return Err(Error::<T, I>::XcmpBeefyRootTargetedNeverSeen.into())
			}

			// Verify stage 1 via grabbing Beefy Root and checking against stage 1
			let (stage_1_proof, stage_1_leaves) = xcmp_proof.stage_1;

			let nodes: Vec<_> = stage_1_leaves
				.clone()
				.into_iter()
				.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
				.collect();

			// TODO: Replace this error with an Error that specifies stage_1 of proof verification failed
			verify_leaves_proof(beefy_root_targeted.into(), nodes, stage_1_proof).map_err(|_| Error::<T, I>::XcmpStage1ProofDoesntVerify)?;

			log::info!(
				target: LOG_TARGET,
				"Verified Stage 1 XCMP Proof Successfully!!!",
			);

			if stage_1_leaves.len() > 1 {
				log::error!("stage_1_leaves length too long {}", stage_1_leaves.len());
				return Err(Error::<T, I>::XcmpStage1LeavesTooLarge.into())
			}

			let stage_1_leaf = &stage_1_leaves[0];
			let stage_1_leaf = stage_1_leaf.clone().into_opaque_leaf();

			let stage_2_leaf: BeefyMmrLeaf<BlockNumberFor<T>, H256, H256, H256> = Decode::decode(&mut &stage_1_leaf.0[..])
				.map_err(|e| {
					log::error!("COULD NOT DECODE LEAF!! WITH ERROR {:?}", e);
					Error::<T, I>::XcmpStage2LeafDoesntDecode
			})?;

			let stage_2_root_from_proof = xcmp_proof.stage_2.root;
			let stage_2_root = stage_2_leaf.leaf_extra;

			if stage_2_root != stage_2_root_from_proof {
				log::error!("Stage_2 Root no match failed to verify Proof!");
				return Err(Error::<T, I>::XcmpStage2RootDoesntMatch.into())
			}

			let stage_2_result = binary_merkle_tree::verify_proof::<Keccak256, _, _>(
				&xcmp_proof.stage_2.root,
				xcmp_proof.stage_2.proof,
				xcmp_proof.stage_2.num_leaves.try_into().unwrap(),
				xcmp_proof.stage_2.leaf_index.try_into().unwrap(),
				Leaf::Value(&xcmp_proof.stage_2.leaf.encode()),
			);

			if !stage_2_result {
				log::error!("Stage 2 proof doesnt verify!!!!!!");
				return Err(Error::<T, I>::XcmpStage2ProofDoesntVerify.into())
			}

			log::info!(
				target: LOG_TARGET,
				"Verified Stage 2 XCMP Proof Successfully!!!",
			);

			let stage_3_root = Self::extract_xcmp_channel_root(xcmp_proof.stage_2.leaf.clone())?;

			if stage_3_root != xcmp_proof.stage_3.root {
				log::error!("Stage_3 Root no match failed to verify Proof!");
				return Err(Error::<T, I>::XcmpStage3RootDoesntMatch.into())
			}

			let stage_3_result = binary_merkle_tree::verify_proof::<Keccak256, _, _>(
				&xcmp_proof.stage_3.root,
				xcmp_proof.stage_3.proof,
				xcmp_proof.stage_3.num_leaves.try_into().unwrap(),
				xcmp_proof.stage_3.leaf_index.try_into().unwrap(),
				&xcmp_proof.stage_3.leaf,
			);

			if !stage_3_result {
				log::error!("Stage 3 proof doesnt verify!!!!!!");
				return Err(Error::<T, I>::XcmpStage3ProofDoesntVerify.into())
			}

			log::info!(
				target: LOG_TARGET,
				"Verified Stage 3 XCMP Proof Successfully!!!",
			);

			let stage_4_root = xcmp_proof.stage_3.leaf;

			let (stage_4_proof, stage_4_leaves) = xcmp_proof.stage_4;

			let nodes: Vec<_> = stage_4_leaves
				.clone()
				.into_iter()
				.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
				.collect();

			verify_leaves_proof(stage_4_root.into(), nodes, stage_4_proof).map_err(|_| Error::<T, I>::XcmpStage4ProofDoesntVerify)?;

			log::info!(
				target: LOG_TARGET,
				"Verified Stage 4 XCMP Proof Successfully!!!",
			);

			// TODO:
			// Now process messages upstream
			// let xcmp_messages = Decode::decode(stage_4_leaves);
			// Send Xcmp Messages upstream to be decoded to XCM messages and processed
			// T::ProcessXcmpMessages(xcmp_messages);

			// Log Event..

			Ok(())
		}
	}

}


impl<T: Config<I>, I: 'static> Pallet<T, I> {

	fn extract_xcmp_channel_root(leaf: ParaLeaf) -> Result<H256, Error<T,I>> {
		// First decode ParaLeaf.head_data into a ParaHeader
		let header: sp_runtime::generic::Header<u32, sp_runtime::traits::BlakeTwo256> =
			sp_runtime::generic::Header::decode(&mut &leaf.head_data[..])
			.map_err(|_e| Error::<T, I>::XcmpStage3HeaderDoesntDecode)?;

		log::debug!("EXTRACTED AND DECODED HEADERRR DANKEEEE!!!!!!{:?}", header);

		// extracting root from digest
		let xcmp_channel_root: H256 = extract_xcmp_channel_merkle_root(&header.digest).ok_or(Error::<T, I>::XcmpStage3CannotBeExtracted)?;

		// Extract the XcmpChannelBinaryMerkleRoot from the Digest
		Ok(xcmp_channel_root)
	}
}

pub struct OnNewRootSatisfier<T>(PhantomData<T>);

impl<T> pallet_mmr::primitives::OnNewRoot<sp_consensus_beefy::MmrRootHash> for OnNewRootSatisfier<T> {
	fn on_new_root(_root: &sp_consensus_beefy::MmrRootHash) {

	}
}

#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct MmrLeaf<BlockNumber, Hash, XcmpMessages> {
	xcmp_msgs: XcmpMessages,
	version: MmrLeafVersion,
	parent_number_and_hash: (BlockNumber, Hash),
}

impl<T: Config<I>, I: 'static> LeafDataProvider for Pallet<T, I> {
	type LeafData = MmrLeaf<BlockNumberFor<T>, T::Hash, XcmpMessages<T, I>>;

	fn leaf_data() -> Self::LeafData {
		let raw_messages = T::XcmpDataProvider::get_xcmp_messages(ParentNumberAndHash::<T>::leaf_data().1, T::ParaIdentifier::get());
		Self::deposit_event(Event::XcmpMessageSent{ msg_hash: <T as frame_system::Config>::Hashing::hash_of(&raw_messages), block_num: ParentNumberAndHash::<T>::leaf_data().0});
		Self::LeafData {
			xcmp_msgs: raw_messages,
			version: T::LeafVersion::get(),
			parent_number_and_hash: ParentNumberAndHash::<T>::leaf_data(),
		}
	}
}

sp_api::decl_runtime_apis! {
	/// API useful for BEEFY light clients.
	pub trait ChannelMerkleApi
	{
		/// Return BinaryMerkle Proof for a particular parachains inclusion in ParaHeader tree
		fn get_xcmp_channels_proof(channel_id: u64) -> Option<ChannelMerkleProof>;
	}
}