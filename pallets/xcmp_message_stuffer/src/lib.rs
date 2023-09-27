#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash};
use sp_consensus_beefy::mmr::MmrLeafVersion;

use frame_support::{dispatch::{DispatchResult}, pallet_prelude::*,};
use frame_system::pallet_prelude::*;
use cumulus_primitives_core::ParaId;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub trait XcmpMessageProvider<Hash> {
	type XcmpMessages: Encode + Decode + scale_info::prelude::fmt::Debug + PartialEq + Clone;

	fn get_xcmp_messages(block_hash: Hash, para_id: ParaId) -> Self::XcmpMessages;
}

type XcmpMessages<T, I> = <<T as crate::Config<I>>::XcmpDataProvider as XcmpMessageProvider<<T as frame_system::Config>::Hash>>::XcmpMessages;


#[frame_support::pallet]
pub mod pallet {
	use super::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config<I: 'static = ()>: frame_system::Config + pallet_mmr::Config<I> {
		type ParaIdentifier: Get<ParaId>;
		type RuntimeEvent: From<Event<Self, I>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type LeafVersion: Get<MmrLeafVersion>;
		type XcmpDataProvider: XcmpMessageProvider<Self::Hash>; // TODO: Needs to be XCMP message commitment or the actual message?
	}

	#[pallet::pallet]
	pub struct Pallet<T, I = ()>(PhantomData<(T, I)>);

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config<I>, I: 'static = ()> {
		SomeEvent,
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T, I = ()> {
		SomeError,
	}

}

pub struct OnNewRootSatisfier<T>(PhantomData<T>);

impl<T> pallet_mmr::primitives::OnNewRoot<sp_consensus_beefy::MmrRootHash> for OnNewRootSatisfier<T> {
	fn on_new_root(root: &sp_consensus_beefy::MmrRootHash) {

	}
}

#[derive(Debug, PartialEq, Eq, Clone, Encode, Decode, TypeInfo)]
pub struct MmrLeaf<BlockNumber, Hash, XcmpMessages> {
	version: MmrLeafVersion,
	xcmp_msgs: XcmpMessages,
	parent_number_and_hash: (BlockNumber, Hash),
}

impl<T: Config<I>, I: 'static> LeafDataProvider for Pallet<T, I> {
	type LeafData = MmrLeaf<BlockNumberFor<T>, T::Hash, XcmpMessages<T, I>>;

	fn leaf_data() -> Self::LeafData {
		Self::LeafData {
			version: T::LeafVersion::get(),
			xcmp_msgs: T::XcmpDataProvider::get_xcmp_messages(ParentNumberAndHash::<T>::leaf_data().1, T::ParaIdentifier::get()), // TODO: This needs to be the current XCMP messages in the current block?
			parent_number_and_hash: ParentNumberAndHash::<T>::leaf_data(),
		}
	}
}

