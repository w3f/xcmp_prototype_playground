use frame_support::{parameter_types, traits::{Everything, Nothing, OriginTrait}, pallet_prelude::{PhantomData, ConstU32}};
use frame_system::EnsureRoot;
use frame_system as system;
pub use sp_core::H256;
pub use sp_runtime::{
	traits::{BlakeTwo256, IdentityLookup, Keccak256},
	BuildStorage,
};

use cumulus_pallet_parachain_system::AnyRelayNumber;
use cumulus_primitives_core::{ParaId, IsSystem};

use sp_core::Hasher;
use sp_consensus_beefy::mmr::MmrLeafVersion;

pub use sp_io::TestExternalities;

use crate::XcmpMessageProvider;

use xcm::{latest::prelude::*, VersionedXcm, WrapVersion, MAX_XCM_DECODE_DEPTH};
use xcm_executor::traits::ConvertOrigin;
use xcm_builder::{CurrencyAdapter, IsConcrete, ParentIsPreset, NativeAsset, FixedWeightBounds};

type Block = frame_system::mocking::MockBlock<Test>;
type Hash = sp_core::H256;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system::{Pallet, Call, Config<T>, Storage, Event<T>},
		Balances: pallet_balances,
		ParachainSystem: cumulus_pallet_parachain_system::{
			Pallet, Call, Config<T>, Storage, Inherent, Event<T>, ValidateUnsigned,
		},
		XcmpQueue: cumulus_pallet_xcmp_queue::{Pallet, Call, Storage, Event<T>},
		MmrParaA: pallet_mmr::<Instance1> = 52,
		MmrParaB: pallet_mmr::<Instance2> = 53,
		MsgStufferA: crate::<Instance1> = 54,
		MsgStufferB: crate::<Instance2> = 55,
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const SS58Prefix: u8 = 42;
}

pub type AccountId = u64;

impl system::Config for Test {
	type BaseCallFilter = Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	type Nonce = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Block = Block;
	type RuntimeEvent = RuntimeEvent;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = SS58Prefix;
	type OnSetCode = cumulus_pallet_parachain_system::ParachainSetCode<Test>;
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

parameter_types! {
	pub const ExistentialDeposit: u64 = 5;
	pub const MaxReserves: u32 = 50;
}

impl pallet_balances::Config for Test {
	type Balance = u64;
	type RuntimeEvent = RuntimeEvent;
	type DustRemoval = ();
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type MaxLocks = ();
	type MaxReserves = MaxReserves;
	type ReserveIdentifier = [u8; 8];
	type RuntimeHoldReason = RuntimeHoldReason;
	type FreezeIdentifier = ();
	type MaxHolds = ConstU32<0>;
	type MaxFreezes = ConstU32<0>;
}

impl cumulus_pallet_parachain_system::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type OnSystemEvent = ();
	type SelfParaId = ();
	type OutboundXcmpMessageSource = XcmpQueue;
	type DmpMessageHandler = ();
	type ReservedDmpWeight = ();
	type XcmpMessageHandler = XcmpQueue;
	type ReservedXcmpWeight = ();
	type CheckAssociatedRelayNumber = AnyRelayNumber;
}

parameter_types! {
	pub const RelayChain: MultiLocation = MultiLocation::parent();
	pub UniversalLocation: InteriorMultiLocation = X1(Parachain(1u32));
	pub UnitWeightCost: Weight = Weight::from_parts(1_000_000, 1024);
	pub const MaxInstructions: u32 = 100;
	pub const MaxAssetsIntoHolding: u32 = 64;
}

/// Means for transacting assets on this chain.
pub type LocalAssetTransactor = CurrencyAdapter<
	// Use this currency:
	Balances,
	// Use this currency when it is a fungible asset matching the given location or name:
	IsConcrete<RelayChain>,
	// Do a simple punn to convert an AccountId32 MultiLocation into a native chain account ID:
	LocationToAccountId,
	// Our chain's account ID type (we can't get away without mentioning it explicitly):
	AccountId,
	// We don't track any teleports.
	(),
>;

pub type LocationToAccountId = (ParentIsPreset<AccountId>,);

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type RuntimeCall = RuntimeCall;
	type XcmSender = XcmRouter;
	// How to withdraw and deposit an asset.
	type AssetTransactor = LocalAssetTransactor;
	type OriginConverter = ();
	type IsReserve = NativeAsset;
	type IsTeleporter = NativeAsset;
	type UniversalLocation = UniversalLocation;
	type Barrier = ();
	type Weigher = FixedWeightBounds<UnitWeightCost, RuntimeCall, MaxInstructions>;
	type Trader = ();
	type ResponseHandler = ();
	type AssetTrap = ();
	type AssetClaims = ();
	type SubscriptionService = ();
	type PalletInstancesInfo = AllPalletsWithSystem;
	type MaxAssetsIntoHolding = MaxAssetsIntoHolding;
	type AssetLocker = ();
	type AssetExchanger = ();
	type FeeManager = ();
	type MessageExporter = ();
	type UniversalAliases = Nothing;
	type CallDispatcher = RuntimeCall;
	type SafeCallFilter = Everything;
	type Aliasers = Nothing;
}

pub type XcmRouter = (
	// XCMP to communicate with the sibling chains.
	XcmpQueue,
);

pub struct SystemParachainAsSuperuser<RuntimeOrigin>(PhantomData<RuntimeOrigin>);
impl<RuntimeOrigin: OriginTrait> ConvertOrigin<RuntimeOrigin>
	for SystemParachainAsSuperuser<RuntimeOrigin>
{
	fn convert_origin(
		origin: impl Into<MultiLocation>,
		kind: OriginKind,
	) -> Result<RuntimeOrigin, MultiLocation> {
		let origin = origin.into();
		if kind == OriginKind::Superuser &&
			matches!(
				origin,
				MultiLocation {
					parents: 1,
					interior: X1(Parachain(id)),
				} if ParaId::from(id).is_system(),
			) {
			Ok(RuntimeOrigin::root())
		} else {
			Err(origin)
		}
	}
}

impl cumulus_pallet_xcmp_queue::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type XcmExecutor = xcm_executor::XcmExecutor<XcmConfig>;
	type ChannelInfo = ParachainSystem;
	type VersionWrapper = ();
	type ExecuteOverweightOrigin = EnsureRoot<AccountId>;
	type ControllerOrigin = EnsureRoot<AccountId>;
	type ControllerOriginConverter = SystemParachainAsSuperuser<RuntimeOrigin>;
	type WeightInfo = ();
	type PriceForSiblingDelivery = ();
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

	fn get_xcmp_messages(block_hash: Hash, para_id: ParaId) -> Self::XcmpMessages {
		use cumulus_pallet_xcmp_queue::OutboundXcmpMessages;
		// TODO: Temporarily we aggregate all the fragments destined to a particular
		// Parachain per block and hash them and stick that into the mmr otherwise need a way
		// of adding multiple MMR leaves per block to the MMR (Which for now means editing the mmr impl?)
		let mut msg_buffer = Vec::new();
		let mut counter = 0u16;
		while let Ok(buffer) = OutboundXcmpMessages::<Test>::try_get(para_id, counter) {
			msg_buffer.extend_from_slice(&buffer[..]);
			counter += 1;
		}

		// TODO: Remove this default and add in some kind of Error/Default if there are no XCMP messages to insert into the MMR?
		BlakeTwo256::hash(&msg_buffer[..])
	}
}

parameter_types! {
	pub ParaAIdentifier: ParaId = ParaId::from(1u32);
}

pub type ParaAChannel = crate::Instance1;
impl crate::Config<ParaAChannel> for Test {
	type ParaIdentifier = ParaAIdentifier;
	type RuntimeEvent = RuntimeEvent;
	type LeafVersion = LeafVersion;
	type XcmpDataProvider = XcmpDataProvider;
}

pub type ParaAMmr = pallet_mmr::Instance1;
impl pallet_mmr::Config<ParaAMmr> for Test {
	const INDEXING_PREFIX: &'static [u8] = b"para_a_mmr";
	type OnNewRoot = crate::OnNewRootSatisfier<Test>;
	type Hashing = Keccak256;
	type LeafData = crate::Pallet<Test, ParaAChannel>;
	type WeightInfo = ();
}

parameter_types! {
	pub ParaBIdentifier: ParaId = ParaId::from(2u32);
}

pub type ParaBChannel = crate::Instance2;
impl crate::Config<ParaBChannel> for Test {
	type ParaIdentifier = ParaBIdentifier;
	type RuntimeEvent = RuntimeEvent;
	type LeafVersion = LeafVersion;
	type XcmpDataProvider = XcmpDataProvider;
}

pub type ParaBMmr = pallet_mmr::Instance2;
impl pallet_mmr::Config<ParaBMmr> for Test {
	const INDEXING_PREFIX: &'static [u8] = b"para_b_mmr";
	type OnNewRoot = crate::OnNewRootSatisfier<Test>;
	type Hashing = Keccak256;
	type LeafData = crate::Pallet<Test, ParaBChannel>;
	type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
	system::GenesisConfig::<Test>::default().build_storage().unwrap().into()
}
