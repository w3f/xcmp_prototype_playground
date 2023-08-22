use crate::{mock::*, Error,};
use frame_support::{assert_noop, assert_ok};

use xcm::{latest::prelude::*, VersionedXcm, WrapVersion, MAX_XCM_DECODE_DEPTH};
use xcm_executor::traits::ConvertOrigin;
use xcm_builder::{CurrencyAdapter, IsConcrete, ParentIsPreset, NativeAsset, FixedWeightBounds};

use sp_consensus_beefy::{
	mmr::MmrLeafVersion,
};

use codec::{Encode, Decode};
use frame_support::traits::{OnInitialize, OnFinalize};

use sp_core::Hasher;

use cumulus_pallet_xcmp_queue::OutboundXcmpMessages;

fn read_mmr_leaf(ext: &mut TestExternalities, key: Vec<u8>) -> MmrLeaf {
	type Node = pallet_mmr::primitives::DataOrHash<Keccak256, MmrLeaf>;
	ext.persist_offchain_overlay();
	let offchain_db = ext.offchain_db();
	offchain_db
		.get(&key)
		.map(|d| Node::decode(&mut &*d).unwrap())
		.map(|n| match n {
			Node::Data(d) => d,
			_ => panic!("Unexpected MMR node."),
		})
		.unwrap()
}

fn init_block(block: u64) {
	System::set_block_number(block);
	Balances::on_initialize(block);
	ParachainSystem::on_initialize(block);
	XcmpQueue::on_initialize(block);
	MmrParaA::on_initialize(block);
	MmrParaB::on_initialize(block);
	MsgStufferA::on_initialize(block);
	MsgStufferB::on_initialize(block);
}

#[test]
fn verify_messages_stuffing_default_works() {
	fn node_offchain_key_a(pos: usize, parent_hash: H256) -> Vec<u8> {
		(<Test as pallet_mmr::Config<ParaAMmr>>::INDEXING_PREFIX, pos as u64, parent_hash).encode()
	}

	fn node_offchain_key_b(pos: usize, parent_hash: H256) -> Vec<u8> {
		(<Test as pallet_mmr::Config<ParaBMmr>>::INDEXING_PREFIX, pos as u64, parent_hash).encode()
	}

	let mut ext = new_test_ext();
	let parent_hash = ext.execute_with(|| {
		init_block(1);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	// ParaA MMR
	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_a(0, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (0_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
		}
	);

	let parent_hash = ext.execute_with(|| {
		init_block(2);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_a(1, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (1_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
		}
	);

	// ParaB MMR
	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_b(0, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (0_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
		}
	);

	let parent_hash = ext.execute_with(|| {
		init_block(2);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_b(1, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (1_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
		}
	);
}

#[test]
fn verify_messages_stuffing_xcmp_messages_works() {
	fn node_offchain_key_a(pos: usize, parent_hash: H256) -> Vec<u8> {
		(<Test as pallet_mmr::Config<ParaAMmr>>::INDEXING_PREFIX, pos as u64, parent_hash).encode()
	}

	fn node_offchain_key_b(pos: usize, parent_hash: H256) -> Vec<u8> {
		(<Test as pallet_mmr::Config<ParaBMmr>>::INDEXING_PREFIX, pos as u64, parent_hash).encode()
	}

	// Dummy Xcm messages
	let message_1 = Xcm(vec![Trap(5)]);
	let message_2 = Xcm(vec![Trap(1)]);

	// XcmpQueue - check dest/msg is valid
	let dest = (Parent, X1(Parachain(5555)));
	let mut dest_wrapper_1 = Some(dest.clone().into());
	let mut dest_wrapper_2 = Some(dest.clone().into());
	let mut msg_wrapper_1 = Some(message_1.clone());
	let mut msg_wrapper_2 = Some(message_2.clone());
	assert!(<XcmpQueue as SendXcm>::validate(&mut dest_wrapper_1, &mut msg_wrapper_1).is_ok());
	assert!(<XcmpQueue as SendXcm>::validate(&mut dest_wrapper_2, &mut msg_wrapper_2).is_ok());

	// check wrapper were consumed
	assert_eq!(None, dest_wrapper_1.take());
	assert_eq!(None, dest_wrapper_2.take());
	assert_eq!(None, msg_wrapper_1.take());
	assert_eq!(None, msg_wrapper_2.take());

	let mut ext = new_test_ext();
	let parent_hash = ext.execute_with(|| {
		init_block(1);
		// Make sure Outbound queue has xcm messages destined to two different parachains
		OutboundXcmpMessages::<Test>::insert(ParaAIdentifier::get(), 0, message_1.encode());
		OutboundXcmpMessages::<Test>::insert(ParaAIdentifier::get(), 1, message_2.encode());
		OutboundXcmpMessages::<Test>::insert(ParaBIdentifier::get(), 0, message_1.encode());
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_a(0, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (0_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
		}
	);

	let parent_hash = ext.execute_with(|| {
		init_block(2);
		assert_eq!(OutboundXcmpMessages::<Test>::contains_key(ParaAIdentifier::get(), 0), true);
		assert_eq!(OutboundXcmpMessages::<Test>::contains_key(ParaAIdentifier::get(), 1), true);
		assert_eq!(OutboundXcmpMessages::<Test>::contains_key(ParaBIdentifier::get(), 0), true);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let single_xcmp = message_1.encode();
	let mut xcmp_aggregate = message_1.encode();
	xcmp_aggregate.extend_from_slice(&message_2.encode());

	// Validate that the correct XCMP message is in the correct MMR
	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_a(1, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (1_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&xcmp_aggregate)
		}
	);

	// Validate that the correct XCMP message is in the correct MMR
	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_b(1, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (1_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&single_xcmp)
		}
	);

}
