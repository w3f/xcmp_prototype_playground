#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash, verify_leaves_proof};
use sp_consensus_beefy::mmr::MmrLeafVersion;

use frame_support::{dispatch::{DispatchResult}, pallet_prelude::*,};
use frame_system::pallet_prelude::*;
use cumulus_primitives_core::ParaId;
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
// type MmrProof<T> = Proof<<T as frame_system::Config>::Hash>;
type MmrProof = Proof<H256>;
type LeafOf<T, I> = <crate::Pallet<T, I> as LeafDataProvider>::LeafData;
type ChannelId = u64;

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
	}

	#[pallet::pallet]
	pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

	/// These are the MMR roots for each open XCMP channel as updated by the Relaychain
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

		// TODO: Retrieve latest valid MmrChannelRoots from Relaychain (Perhaps this is done in on_initialize)

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
	}

}

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