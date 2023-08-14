use frame_support::{parameter_types, traits::Everything};
use frame_system as system;
pub use sp_core::H256;
pub use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup, Keccak256},
	BuildStorage,
};

use sp_core::Hasher;
use sp_consensus_beefy::mmr::MmrLeafVersion;

pub use sp_io::TestExternalities;

use crate::XcmpMessageProvider;

type Block = frame_system::mocking::MockBlock<Test>;
type Hash = sp_core::H256;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
		Mmr: pallet_mmr,
		MsgStuffer: crate::{Pallet, Call, Storage, Event<T>},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

impl system::Config for Test {
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Nonce = u64;
	type Hash = Hash;
	type Hashing = BlakeTwo256;
	type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = ();
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

parameter_types! {
	/// Version of the produced MMR leaf.
	///
	/// The version consists of two parts;
	/// - `major` (3 bits)
	/// - `minor` (5 bits)
	///
	/// `major` should be updated only if decoding the previous MMR Leaf format from the payload
	/// is not possible (i.e. backward incompatible change).
	/// `minor` should be updated if fields are added to the previous MMR Leaf, which given SCALE
	/// encoding does not prevent old leafs from being decoded.
	///
	/// Hence we expect `major` to be changed really rarely (think never).
	/// See [`MmrLeafVersion`] type documentation for more details.
	pub LeafVersion: MmrLeafVersion = MmrLeafVersion::new(2, 8);
}

pub type MmrLeaf = crate::MmrLeaf<
	frame_system::pallet_prelude::BlockNumberFor<Test>,
	<Test as frame_system::Config>::Hash,
	<Test as frame_system::Config>::Hash
>;

pub struct XcmpDataProvider;
impl XcmpMessageProvider<Hash> for XcmpDataProvider {
	type XcmpMessages = Hash;

	fn get_xcmp_message(block_hash: Hash) -> Self::XcmpMessages {
		// TODO: Temporarily to "Mock" the Xcmp message we just place the hash of the block hash
		<BlakeTwo256 as Hasher>::hash(block_hash.as_bytes())
	}
}

impl crate::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type LeafVersion = LeafVersion;
	type XcmpDataProvider = XcmpDataProvider;
}

impl pallet_mmr::Config for Test {
	const INDEXING_PREFIX: &'static [u8] = pallet_mmr::primitives::INDEXING_PREFIX;
	type OnNewRoot = crate::OnNewRootSatisfier<Test>;
	type Hashing = Keccak256;
	type LeafData = crate::Pallet<Test>;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	system::GenesisConfig::<Test>::default().build_storage().unwrap().into()
}
