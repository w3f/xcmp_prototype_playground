## XCMP (Cross-Chain Message Passing)

### Overview
In a scenario where `ParaA` wants to send a message to `ParaB` (`ParaA -> ParaB`), various steps and data structures come into play. 

## Goals with this Repo

- Better understand an efficient implementations of different XCMP approaches(proof sizes, overall onchain benchmarking, code complexity)
- Achieve consensus on an XCMP design and approach to put forward to the community (By showing pros and cons of a few different approaches)

### Prototypes for two approaches which are the following:
    1. Msgs are unordered and no guranteed delivery(Use MMR approach here)
    2. Msgs are ordered (Use msg hash chain)

## Approach 1

### Data Structure Format and Flow

ParaA MSGab -> Message MMR -> MMR root -> XCMP trie -> XCMP trie root -> Parachain State trie -> Parachain state root -> Relay state trie -> Relay Root

Q:
    Does each MMR peak get added to the XCMP trie? Or just each MMR Root(after bagging peaks). What else can be stored in the XCMP trie?


### XCMPChannel Trie Contents

- MMRab root, MMRac root, etc.: Each parachains XCMP channel's MMR root.

### XCMPTrie
- This contains every Parachain's XCMPChannelTrieRoot

### Flow of Messages

#### ParaA(Sender):

- Sends an XCM message as usual.
- Adds the message as a leaf into one of its message XcmpChannelMmrs.
- The message MMR root (or all peaks?) gets stored inside the XCMP dedicated trie.

#### Relayer (Having full nodes of `ParaA`, `ParaB`, Relaychain):

1. **Detection of Destination**:
   - `Relayer` module detects a message from `ParaA` to `ParaB` is sent.

2. **Proof Construction**:
   - Constructs the proof as `ParaA proof` = (messages + mmr membership proof).

3. **Proof Submission**:
   - Submits proof to `ParaB`'s XCMP extrinsic labeled `submit_xcmp_proof(leaves, channel_id)`.
  
#### ParaB(Receiver):
  
1. **Proof Verification**:
   -  Because the receiver can track the XCMPChannelTrieRoot changes on the Relaychain the verifier stores the latest XcmpChannelMmr roots in its state
     	Because a parachain can have multiple Channels it stores the particular XcmpChannelMmrRoots indexed by a `channel_id`
   -  When the Relayer submits a proof of a particular of some particular messages the receiving chain can check those messages(leaves of the mmr) against its current
     	XcmpChannelMmrRoot for the particular `channel_id`

```rust

pub struct RelayerProof {
    message_proof: Proof<H256>,
    channel_id: u64,
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


# Building:

#### Clone
```bash 
git clone git@github.com:w3f/xcmp_prototype_playground.git && cd xcmp_prototype_playground
```

#### Run Build script
```bash
chmod +x build.sh && ./build.sh
```

#### Clone and compile current XCMP supported Polkadot
```bash
cd ../ && \
git clone https://github.com/coax1d/polkadot-sdk/tree/xcmp_customized_sdk && \
cd polkadot-sdk && \
cargo build --release -p polkadot
```

#### Move all necessary Polkadot binaries to bin directory with other collator binaries 
```bash
cd ../xcmp_prototype_playground && \
cp ../polkadot-sdk/target/release/polkadot bin/polkadot && \
cp ../polkadot-sdk/target/release/polkadot-execute-worker bin/polkadot-execute-worker && \
cp ../polkadot-sdk/target/release/polkadot-prepare-worker bin/polkadot-prepare-worker
```

# Running Entire setup E2E:

#### Open Terminal
```bash
cd xcmp_prototype_playground
```

#### Run zombienet
```bash
zombienet-macos spawn -p native zombienet/config.toml
```

#### Open new terminal wait ~ 1 minute for collators to be onboarded to Relaychain
```bash
./target/release/xcmp_relayer
```
