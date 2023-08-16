use crate::{mock::*, Error,};
use frame_support::{assert_noop, assert_ok};

use sp_consensus_beefy::{
	mmr::MmrLeafVersion,
};

use codec::{Encode, Decode};
use frame_support::traits::OnInitialize;

use sp_core::Hasher;

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
	Mmr::on_initialize(block);
	MsgStuffer::on_initialize(block);
}

#[test]
fn verify_messages_stuffing_works() {
	fn node_offchain_key(pos: usize, parent_hash: H256) -> Vec<u8> {
		(<Test as pallet_mmr::Config>::INDEXING_PREFIX, pos as u64, parent_hash).encode()
	}

	let mut ext = new_test_ext();
	let parent_hash = ext.execute_with(|| {
		init_block(1);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key(0, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (0_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(parent_hash.as_bytes())
		}
	);

	let parent_hash = ext.execute_with(|| {
		init_block(2);
		<frame_system::Pallet<Test>>::parent_hash()
	});

	let mmr_leaf = read_mmr_leaf(&mut ext, node_offchain_key(1, parent_hash));
	assert_eq!(
		mmr_leaf,
		MmrLeaf {
			version: MmrLeafVersion::new(2, 8),
			parent_number_and_hash: (1_u64, H256::repeat_byte(0x45)),
			xcmp_msgs: <BlakeTwo256 as Hasher>::hash(parent_hash.as_bytes())
		}
	);
}
