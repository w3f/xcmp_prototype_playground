#!/usr/bin/env sh

# Function to toggle the value between 1000 and 1001 on a specific line
toggle_paraid() {
    local line=$1
    local file=$2
    local current_value=$(sed -n "${line}p" "$file" | grep -o '[1000|1001]*')

    if [ "$current_value" = "1000" ]; then
        sed -i'' -e "${line}s/1000/1001/" "$file"
    elif [ "$current_value" = "1001" ]; then
        sed -i'' -e "${line}s/1001/1000/" "$file"
    fi
}

echo "Creating bin for storing binaries"
# Check if the bin directory exists, create it if it does not
if [ ! -d "bin" ]; then
    echo "bin directory does not exist. Creating one..."
    mkdir bin
    if [ $? -ne 0 ]; then
        echo "Failed to create bin directory"
        exit 1
    fi
fi

echo "Building parachain runtimes first.."
sleep 1

cargo build --release -p parachain-template-node -p pallet-xcmp-message-stuffer -p parachain-template-runtime

if [ $? -eq 0 ]; then
    echo "Building Parachain Succeeded"

    cp ./target/release/parachain-template-node ./bin/parachain-template-node-v1.1.0-1000
    if [ $? -ne 0 ]; then
        echo "Failed to copy parachain-template-node to bin directory"
        exit 1
    fi
else
    echo "Building Parachain Failed"
    exit 1
fi

toggle_paraid 161 node/src/chain_spec.rs
toggle_paraid 177 node/src/chain_spec.rs

echo "Building second parachain runtime for paraid 1001"
sleep 1

cargo build --release -p parachain-template-node -p pallet-xcmp-message-stuffer -p parachain-template-runtime

if [ $? -eq 0 ]; then
    echo "Building Parachain Succeeded"

    cp ./target/release/parachain-template-node ./bin/parachain-template-node-v1.1.0-1001
    if [ $? -ne 0 ]; then
        echo "Failed to copy parachain-template-node to bin directory"
        exit 1
    fi
else
    echo "Building Parachain Failed"
    exit 1
fi

echo "Resetting para_ids for next build"
toggle_paraid 161 node/src/chain_spec.rs
toggle_paraid 177 node/src/chain_spec.rs

echo "Starting Parachain Node to get Metadata"

nohup ./target/release/parachain-template-node --dev --rpc-port 54887 &

PARACHAIN_PID=$!

echo "Parachain started with PID: $PARACHAIN_PID"
sleep 5 

echo "Now building Relayer"
cargo build --release -p xcmp_relayer

if [ $? -eq 0 ]; then
    echo "Building Parachain Succeeded"
else
    echo "Building Parachain Failed"
    exit 1
fi

kill $PARACHAIN_PID

sleep 2

if kill -0 $PARACHAIN_PID 2>/dev/null; then
    echo "Parachain node could not be killed"
else
    echo "Parachain node killed successfully"
fi

echo "Build complete!!"
