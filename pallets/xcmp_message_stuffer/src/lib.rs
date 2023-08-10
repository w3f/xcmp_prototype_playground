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


#[frame_support::pallet]
pub mod pallet {
	use super::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_mmr::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type LeafVersion: Get<MmrLeafVersion>;
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

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// An example dispatchable that takes a singles value as a parameter, writes the value to
		/// storage and emits an event. This function must be dispatched by a signed extrinsic.
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
pub struct MmrLeaf<BlockNumber, Hash> {
	version: MmrLeafVersion,
	xcmp_msg: Vec<u8>, // TODO: Replace with some XCMP message type I assume..
	parent_number_and_hash: (BlockNumber, Hash),
}

impl<T: Config> LeafDataProvider for Pallet<T> {
	type LeafData = MmrLeaf<BlockNumberFor<T>, T::Hash>;

	fn leaf_data() -> Self::LeafData {
		Self::LeafData {
			version: T::LeafVersion::get(),
			xcmp_msg: Vec::new(),
			parent_number_and_hash: ParentNumberAndHash::<T>::leaf_data(),
		}
	}
}

