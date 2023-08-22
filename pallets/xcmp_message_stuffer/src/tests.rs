use crate::{mock::*, Error,};
use frame_support::{assert_noop, assert_ok};

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

fn finalize_queue(block: u64) {
	XcmpQueue::on_finalize(block);
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

	let xcm_message_frag = vec![1,2,3];
	let mut aggregate = xcm_message_frag.clone();
	aggregate.extend_from_slice(&vec![1,2,3]);

	let mut ext = new_test_ext();
	let parent_hash = ext.execute_with(|| {
		init_block(1);
		OutboundXcmpMessages::<Test>::insert(ParaAIdentifier::get(), 0, xcm_message_frag.clone());
		// OutboundXcmpMessages::<Test>::insert(ParaAIdentifier::get(), 1, xcm_message_frag.clone());
		Balances::force_set_balance(RuntimeOrigin::signed(1u64), 1, 1000);
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
		<frame_system::Pallet<Test>>::parent_hash()
	});

	// let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key_a(0, parent_hash));
	// assert_eq!(
	// 	mmr_leaf,
	// 	MmrLeaf {
	// 		version: MmrLeafVersion::new(2, 8),
	// 		parent_number_and_hash: (0_u64, H256::repeat_byte(0x45)),
	// 		xcmp_msgs: <BlakeTwo256 as Hasher>::hash(&Vec::new())
	// 	}
	// );

}
