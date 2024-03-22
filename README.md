## XCMP (Cross-Chain Message Passing)

### Overview
In a scenario where `ParaA` wants to send a message to `ParaB` (`ParaA -> ParaB`), various steps and data structures come into play. 

## Goals with this Repo

- Better understand an efficient implementations of different XCMP approaches(proof sizes, overall onchain benchmarking, code complexity)
- Achieve consensus on an XCMP design and approach to put forward to the community (By showing pros and cons of a few different approaches)

## Approach

### The proofs which are constructed via a Relayer involve a 4 tiered authenticated datastructure using 2 Binary Merkle Trees and 2 MMRs.

### ChannelMessageMmr
- This is one of many MMRs which XCMP messages are stored into. There is a single XCMP message per block so each index can be thought of as the message contents for a particular block.

### XCMPChannelBinaryMerkleTree Contents

- MMRab root, MMRac root, etc.: Each receipient which a sender has an open channel with has a dedicated `ChannelMessageMmr`.
This each one of these `ChannelMessageMmrRoots` are bundled up into a Binary Merkle Tree whos root is put into the `Digest` of each Parachains BlockHeader

### Beefy MMR

- Every Leaf of the Beefy Mmr contains a committment every `parachains block header` in the form of a `BinaryMerkleRoot`

### Combining

By combining these 4 tiers of Merkle structures together we have a proof which from a single message commitment on some parachain sender `X` we can compare it against another parachain receiver `Y`. This requires a latest Beefy Root (Which lives on the Polkadot Relaychain) to be inserted into each parachains state for proof verification.

### Insert Diagram of 4 tiered merkle structure

### Flow of Messages

#### ParaA(Sender):

- Sends an XCM message as usual.
- Adds the message as a leaf into one of its message XcmpChannelMessageMmrs.
- The message MMR root gets stored inside the XCMPChannelBinaryMerkleTree.

#### Relayer (Having full nodes of `ParaA`, `ParaB`, Relaychain):

1. **Detection of Destination**:
   - `Relayer` module detects a message from `ParaA` to `ParaB` is sent.

2. **Proof Construction**:
   - Constructs 4 tiered MerkleProof

3. **Proof Submission**:
   - Submits proof to `ParaB`'s XCMP extrinsic labeled `submit_xcmp_proof(proof, channel_id)`.
  
#### ParaB(Receiver):
  
**Proof Verification**:
   - Because receipient has the Beefy Mmr Root(The top of the Merkle Path) it can unravel the proof and compare against this root.

#### Incentivization of Relayer:

1. Any parachain that opens a channel with another parachain could potentially run relayer nodes.
2. The XCM message sender provides a percentage fee to the relayer.

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
