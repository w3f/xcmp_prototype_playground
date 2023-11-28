#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash, verify_leaves_proof};
use sp_consensus_beefy::mmr::MmrLeafVersion;

use frame_support::{dispatch::{DispatchResult}, pallet_prelude::*,};
use frame_system::pallet_prelude::*;
use cumulus_primitives_core::{ParaId, GetBeefyRoot};
use sp_runtime::traits::{Hash as HashT, Keccak256};
use sp_core::H256;

use sp_mmr_primitives::{Proof, EncodableOpaqueLeaf, DataOrHash};
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

type XcmpMessages<T, I> = <<T as crate::Config<I>>::XcmpDataProvider as XcmpMessageProvider<<T as frame_system::Config>::Hash>>::XcmpMessages;
type MmrProof = Proof<H256>;
type LeafOf<T, I> = <crate::Pallet<T, I> as LeafDataProvider>::LeafData;
type ChannelId = u64;
type BinaryMerkleProof = ();


#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct XcmpProof {
	// TODO: Probably should rename each of these stages to some fancy name
	// TODO: Remove tuples
	pub stage_1: (MmrProof, Vec<EncodableOpaqueLeaf>),
	pub stage_2: BinaryMerkleProof,
	pub stage_3: BinaryMerkleProof,
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
	}

	#[pallet::pallet]
	pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

	/// These are the MMR roots for each open XCMP channel as verified against current Relay Chain Beefy Root
	#[pallet::storage]
	#[pallet::getter(fn xcmp_channel_roots)]
	pub type XcmpChannelRoots<T: Config<I>, I: 'static = ()> = StorageMap<_, Identity, ChannelId, H256, OptionQuery>;

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
	}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {

		// TODO: This will
		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn submit_xcmp_proof(origin: OriginFor<T>, mmr_proof: MmrProof, leaves: Vec<EncodableOpaqueLeaf>, channel_id: u64) -> DispatchResult {
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
		/// the relaychain updated XCMPTrie
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
		pub fn submit_big_proof(origin: OriginFor<T>, xcmp_proof: XcmpProof) -> DispatchResult {
			ensure_signed(origin)?;

			log::info!(
				target: LOG_TARGET,
				"Called submit big proof",
			);
			// Verify stage 1 via grabbing Beefy Root and checking against stage 1
			let (stage_1_proof, stage_1_leaves) = xcmp_proof.stage_1;
			// Get currenty Beefy Mmr root
			let beefy_root = T::BeefyRootProvider::get_root().unwrap_or(Default::default());

			log::info!(
				target: LOG_TARGET,
				"Current on chain Beefy Root is: {:?}",
				beefy_root
			);

			let nodes: Vec<_> = stage_1_leaves
				.clone()
				.into_iter()
				.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
				.collect();

			// TODO: Replace this error with an Error that specifies stage_1 of proof verification failed
			verify_leaves_proof(beefy_root.into(), nodes, stage_1_proof).map_err(|_| Error::<T, I>::XcmpProofNotValid)?;

			// Verify stage 2..
			// grab ParaHeader root from stage_1_proof
			// let para_header_root = Decode::decode(stage_1_leaves)
			// let (stage_2_proof, stage_2_leaves) = xcmp_proof.stage_2;

			// These are different leaves they arent the MmrLeaves they are Binary Merkle Leaves
			// This will be a bit different but same rough idea as the Mmr
			// let nodes: Vec<_> = stage_2_leaves
			// 	.clone()
			// 	.into_iter()
			// 	.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
			// 	.collect();

			// binary merkle proof verification of para_header_root against stage_2_proof(leaves are (para_id, para_header))
			// verify_proof(root, nodes, stage_2_proof);

			// let (para_id, para_header) = Decode::decode(stage_2_leaves);
			// Check channels storage to make sure this ParaId is someone that we support
			// if !XcmpChannels::<T>::exists(para_id) {
					// return Error::<T>::XcmpProofNoChannelWithSender
			// }

			// Verify stage 3..
			// extract xcmp_root from paraheader..
			// let xcmp_root = extract(para_header)
			// let (stage_3_proof, stage_3_leaves) = xcmp_proof.stage_3;

			// These are different leaves they arent the MmrLeaves they are Binary Merkle Leaves
			// This will be a bit different but same rough idea as the Mmr
			// let nodes: Vec<_> = stage_3_leaves
			// 	.clone()
			// 	.into_iter()
			// 	.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
			// 	.collect();

			// binary merkle proof verification of xcmp_root against stage_3_proof(mmr_root_from_sender)
			// verify_proof(xcmp_root, nodes, stage_3_proof)?;

			// Verify stage 4..
			// let mmr_root = Decode::decode(stage_3_leaves);
			// let (stage_4_proof, stage_4_leaves) = xcmp_proof.stage_4;

			// let nodes: Vec<_> = stage_4_leaves
			// 	.clone()
			// 	.into_iter()
			// 	.map(|leaf|DataOrHash::<Keccak256, _>::Data(leaf.into_opaque_leaf()))
			// 	.collect();

			// TODO: Replace this error with an Error that specifies stage_4 of proof verification failed
			// verify_leaves_proof(mmr_root.into(), nodes, stage_4_proof).map_err(|_| Error::<T, I>::XcmpProofNotValid)?;

			// Now process messages upstream
			// let xcmp_messages = Decode::decode(stage_4_leaves);
			// Send Xcmp Messages upstream to be decoded to XCM messages and processed
			// T::ProcessXcmpMessages(xcmp_messages);

			// Log Event..

			Ok(())
		}
	}

}

// TODO: Add Inherent which can update the current `XcmpChannelRoots` against the current BeefyMmrRoot

pub struct OnNewRootSatisfier<T>(PhantomData<T>);

impl<T> pallet_mmr::primitives::OnNewRoot<sp_consensus_beefy::MmrRootHash> for OnNewRootSatisfier<T> {
	fn on_new_root(root: &sp_consensus_beefy::MmrRootHash) {

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