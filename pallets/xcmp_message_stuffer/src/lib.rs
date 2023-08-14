#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash};
use sp_consensus_beefy::mmr::MmrLeafVersion;

use frame_support::{dispatch::{DispatchResult, Vec}, pallet_prelude::*,};
use frame_system::pallet_prelude::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub trait XcmpMessageProvider<Hash> {
	type XcmpMessages: Encode + Decode + frame_support::dispatch::fmt::Debug + PartialEq + Clone;

	fn get_xcmp_message(block_hash: Hash) -> Self::XcmpMessages;
}

type XcmpMessages<T> = <<T as crate::Config>::XcmpDataProvider as XcmpMessageProvider<<T as frame_system::Config>::Hash>>::XcmpMessages;


#[frame_support::pallet]
pub mod pallet {
	use super::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_mmr::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type LeafVersion: Get<MmrLeafVersion>;
		type XcmpDataProvider: XcmpMessageProvider<Self::Hash>; // TODO: Needs to be XCMP message commitment or the actual message?
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		SomeEvent,
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		SomeError,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {

		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn verify_proof(origin: OriginFor<T>) -> DispatchResult {
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
	version: MmrLeafVersion,
	xcmp_msg: XcmpMessages,
	parent_number_and_hash: (BlockNumber, Hash),
}

impl<T: Config> LeafDataProvider for Pallet<T> {
	type LeafData = MmrLeaf<BlockNumberFor<T>, T::Hash, XcmpMessages<T>>;

	fn leaf_data() -> Self::LeafData {
		Self::LeafData {
			version: T::LeafVersion::get(),
			xcmp_msg: T::XcmpDataProvider::get_xcmp_message(ParentNumberAndHash::<T>::leaf_data().1), // TODO: This needs to be the current XCMP messages in the current block?
			parent_number_and_hash: ParentNumberAndHash::<T>::leaf_data(),
		}
	}
}

