## XCMP (Cross-Chain Message Passing)

### Overview
In a scenario where `ParaA` wants to send a message to `ParaB` (`ParaA -> ParaB`), various steps and data structures come into play. 

### Data Structure Format and Flow

ParaA MSGab -> Message MMR -> MMR root -> XCMP trie -> XCMP trie root -> Parachain State trie -> Parachain state root -> Relay state trie -> Relay Root

Q:
    Does each MMR peak get added to the XCMP trie? Or just each MMR Root(after bagging peaks). What is stored in the XCMP trie?


### XCMP Trie Contents

- MMRab root, MMRbc root, etc.: Each XCMP channel's MMR root.
- Polkadot XCMP trie root
- KSM XCMP trie root

### Flow of Messages

#### ParaA:

- Sends an XCM message as usual.
- Adds a commitment of the message (plus an index?) into its message MMR.
- The message MMR root (or all peaks?) gets stored inside the XCMP dedicated trie.

#### Relayer (Having full nodes of `ParaA`, `ParaB`, Relaychain):

1. **Detection of Destination**:
   - `ParaA` module detects the message is destined for `ParaB`.
   - **Query**: How? Possibly by reading XCM message MultiLocations?

2. **Proof Construction**:
   - Constructs the proof as `ParaA proof` = (message + mmr root + membership proof (via MMR nodes and peaks)).
   - Relay proof = (membership proof (ParaA message + MMR peak)).
   
   - **Open Query**: How does the relayer detect and/or receive the messages?

3. **Proof Submission**:
   - Submits proof to `ParaB`'s XCMP extrinsic labeled `verify_xcmp_message(RelayerProof)`.

```rust
pub struct RelayerProof {
    message: XcmMessage,
    mmr_root: Hash,
    message_proof: MmrProof,
    relay_proof: RelayProof, 
}

pub struct MmrProof {
    mmr_peaks: Vec<MmrNode>,
    mmr_nodes: Vec<MmrNode>,
}

pub struct RelayProof {
    relay_root: Hash,
    relay_nodes: Vec<RelayNode>,
}
```

#### Incentivization of Relayer:

1. Any parachain that opens a channel with another parachain could potentially run relayer nodes.
2. The XCM message sender provides a percentage fee to the relayer.


### ParaB:

#### verify_xcmp_message(RelayerProof)`

- verifies MMR nodes against mmr_roots
- verifies Relay merkle nodes against relay root
- If verification successful accepts XCM message and sends up the stack to XcmExecutor, XcmRouter?

- **Open questions:
            Where does message go from here? (Check UMP/DMP/HRMP code)


