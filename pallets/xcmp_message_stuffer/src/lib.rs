#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use pallet_mmr::{LeafDataProvider, ParentNumberAndHash};
use sp_consensus_beefy::mmr::MmrLeafVersion;

use frame_support::{dispatch::{DispatchResult}, pallet_prelude::*,};
use frame_system::pallet_prelude::*;
use cumulus_primitives_core::ParaId;
use sp_runtime::traits::{Hash as HashT};

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
// TODO: Need the MmrProof to beable to seperate each leaf such that we can decode each XCMP message aggregate
type MmrProof = ();

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
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

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config<I>, I: 'static = ()> {
		XcmpMessageSent {
			msg_hash: T::Hash,
			block_num: BlockNumberFor<T>,
		},
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T, I = ()> {
		XcmpProofNotValid,
		XcmpProofAccepted,
	}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {

		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn verify_xcmp_proof(origin: OriginFor<T>, mmr_proof: MmrProof, mmr_root: T::Hash, relay_proof: ()) -> DispatchResult {
			T::RelayerOrigin::ensure_origin(origin)?;

			// TODO:
			// 1.) Verify MmrProof by calling verify with MmrRoot and MmrProof
			// 2.) Verify relay proof passes (para_block which carries these messages is included)
			// 3.) if passes check then start process of decoding the XCMP blob into its XCM components
			// 4.) Process XCM messages

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
	xcmp_msgs: XcmpMessages,
	parent_number_and_hash: (BlockNumber, Hash),
}

impl<T: Config<I>, I: 'static> LeafDataProvider for Pallet<T, I> {
	type LeafData = MmrLeaf<BlockNumberFor<T>, T::Hash, XcmpMessages<T, I>>;

	fn leaf_data() -> Self::LeafData {
		let raw_messages = T::XcmpDataProvider::get_xcmp_messages(ParentNumberAndHash::<T>::leaf_data().1, T::ParaIdentifier::get());
		Self::deposit_event(Event::XcmpMessageSent{ msg_hash: <T as frame_system::Config>::Hashing::hash_of(&raw_messages), block_num: ParentNumberAndHash::<T>::leaf_data().0});
		Self::LeafData {
			version: T::LeafVersion::get(),
			xcmp_msgs: raw_messages,
			parent_number_and_hash: ParentNumberAndHash::<T>::leaf_data(),
		}
	}
}