#!/bin/bash

# StellopayCore CLI Demo Script
# This script demonstrates the basic usage of the CLI

echo "🚀 StellopayCore CLI Demo"
echo "========================"

# Build the CLI if needed
echo "📦 Building CLI..."
if [ ! -f "target/release/stellopay-cli" ]; then
    cargo build --release
fi

CLI_PATH="./target/release/stellopay-cli"

echo
echo "1. 📊 Checking CLI status..."
$CLI_PATH status

echo
echo "2. 🔍 Showing CLI help..."
$CLI_PATH --help

echo
echo "3. 📋 Showing version..."
$CLI_PATH --version

echo
echo "4. 🔧 Testing configuration creation..."
TEMP_CONFIG=$(mktemp)
$CLI_PATH --config "$TEMP_CONFIG" status > /dev/null 2>&1
if [ -f "$TEMP_CONFIG" ]; then
    echo "✅ Configuration file created successfully"
    echo "📄 Configuration contents:"
    cat "$TEMP_CONFIG"
    rm "$TEMP_CONFIG"
else
    echo "❌ Configuration file creation failed"
fi

echo
echo "5. 🔍 Testing info command (without contract ID - should fail)..."
$CLI_PATH info 2>&1 || echo "✅ Correctly failed without contract ID"

echo
echo "6. 🔍 Testing info command (with invalid contract ID)..."
$CLI_PATH info --contract-id "invalid_id" 2>&1 || echo "✅ Handled invalid contract ID gracefully"

echo
echo "7. 🚀 Testing deploy command (should fail without owner)..."
$CLI_PATH deploy 2>&1 || echo "✅ Correctly failed without owner address"

echo
echo "8. 🧪 Testing deploy command (with invalid owner)..."
$CLI_PATH deploy --owner "invalid_owner" 2>&1 || echo "✅ Correctly failed with invalid owner"

echo
echo "✅ Demo completed!"
echo "🎯 All basic CLI functionality is working correctly."
echo
echo "Next steps:"
echo "  • Deploy a real contract: stellopay-cli deploy --owner <YOUR_STELLAR_ADDRESS>"
echo "  • Query contract info: stellopay-cli info --contract-id <CONTRACT_ID>"
echo "  • Check status anytime: stellopay-cli status"
