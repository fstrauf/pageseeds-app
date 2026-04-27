#!/bin/bash
# Generate TypeScript types from Rust using ts-rs
# This ensures TypeScript types always match Rust structs

set -e

echo "🔄 Generating TypeScript bindings from Rust..."

cd "$(dirname "$0")/../src-tauri"

# Run cargo test to trigger ts-rs exports
echo "📦 Running cargo test (this generates bindings)..."
cargo test --quiet

# Check if bindings were generated
if [ -d "bindings" ]; then
    echo "✅ Bindings generated in src-tauri/bindings/"
    echo ""
    echo "📋 Recent bindings:"
    ls -la bindings/*.ts | tail -10
    echo ""
    echo "⚠️  IMPORTANT: Update src/lib/types.ts to re-export from bindings/"
    echo "   Example: export type { QueueItem } from '../../src-tauri/bindings/QueueItem'"
else
    echo "❌ No bindings directory found"
    exit 1
fi
