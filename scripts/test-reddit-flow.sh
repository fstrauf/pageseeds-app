#!/bin/bash
#
# Reddit Flow End-to-End Test Runner
#
# This script runs the comprehensive Reddit search flow tests.
# Tests require real API access (Kimi CLI and Reddit API).
#
# Usage:
#   ./scripts/test-reddit-flow.sh [test_name]
#
# Examples:
#   ./scripts/test-reddit-flow.sh                    # Run all tests
#   ./scripts/test-reddit-flow.sh config             # Run config parsing test
#   ./scripts/test-reddit-flow.sh full               # Run full flow test
#   ./scripts/test-reddit-flow.sh json               # Run JSON extraction test

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TAURI_DIR="$PROJECT_DIR/src-tauri"

cd "$TAURI_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

# Check prerequisites
check_prerequisites() {
    print_header "Checking Prerequisites"
    
    # Check if cargo is installed
    if ! command -v cargo &> /dev/null; then
        print_error "Cargo not found. Please install Rust."
        exit 1
    fi
    print_success "Cargo found"
    
    # Check if kimi CLI is installed
    if ! command -v kimi &> /dev/null; then
        print_error "Kimi CLI not found. Install with: pip install kimi-cli"
        print_error "Or visit: https://github.com/MoonshotAI/kimi-cli"
        exit 1
    fi
    print_success "Kimi CLI found"
    
    # Check kimi authentication
    print_warning "Note: Kimi requires authentication. Ensure you've run 'kimi auth'"
    
    echo ""
}

# Run the JSON extraction unit test (fast, no APIs needed)
run_json_test() {
    print_header "Test: JSON Extraction (Unit Test)"
    echo "This test verifies JSON extraction from various Kimi output formats."
    echo "No external APIs required."
    echo ""
    
    if cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output -- --nocapture; then
        print_success "JSON extraction test passed!"
        return 0
    else
        print_error "JSON extraction test failed!"
        return 1
    fi
}

# Run the config parsing test with real Kimi
run_config_test() {
    print_header "Test: Reddit Config Parsing with Real Kimi"
    echo "This test:"
    echo "  1. Creates a test project with sample reddit_config.md"
    echo "  2. Calls the real Kimi agent to parse the config"
    echo "  3. Verifies the parsed JSON output"
    echo ""
    print_warning "This test requires:"
    echo "  - Kimi CLI to be authenticated"
    echo "  - ~30 seconds to complete (Kimi processing time)"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo ""
    
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        if cargo test --test reddit_e2e_test test_reddit_config_parsing_with_real_kimi -- --ignored --nocapture; then
            print_success "Config parsing test passed!"
            return 0
        else
            print_error "Config parsing test failed!"
            return 1
        fi
    else
        print_warning "Test skipped by user"
        return 2
    fi
}

# Run the full flow test with real APIs
run_full_flow_test() {
    print_header "Test: Full Reddit Flow with Real APIs"
    echo "This test:"
    echo "  1. Creates a test project with config files"
    echo "  2. Calls Kimi to parse reddit_config.md"
    echo "  3. Searches Reddit API using parsed parameters"
    echo "  4. Verifies the complete flow"
    echo ""
    print_warning "This test requires:"
    echo "  - Kimi CLI to be authenticated"
    echo "  - Internet connection for Reddit API"
    echo "  - ~60 seconds to complete"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo ""
    
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        if cargo test --test reddit_e2e_test test_full_reddit_flow_with_real_apis -- --ignored --nocapture; then
            print_success "Full flow test passed!"
            return 0
        else
            print_error "Full flow test failed!"
            return 1
        fi
    else
        print_warning "Test skipped by user"
        return 2
    fi
}

# Run all tests
run_all_tests() {
    print_header "Running All Reddit Flow Tests"
    
    local failed=0
    
    # Run unit test first (always runs)
    if ! run_json_test; then
        ((failed++))
    fi
    
    echo ""
    
    # Run integration tests with real APIs
    if ! run_config_test; then
        if [ $? -eq 1 ]; then
            ((failed++))
        fi
    fi
    
    echo ""
    
    if ! run_full_flow_test; then
        if [ $? -eq 1 ]; then
            ((failed++))
        fi
    fi
    
    echo ""
    print_header "Test Summary"
    
    if [ $failed -eq 0 ]; then
        print_success "All tests passed!"
        return 0
    else
        print_error "$failed test(s) failed"
        return 1
    fi
}

# Main
check_prerequisites

case "${1:-all}" in
    json|unit)
        run_json_test
        ;;
    config|parse)
        run_config_test
        ;;
    full|e2e|integration)
        run_full_flow_test
        ;;
    all)
        run_all_tests
        ;;
    *)
        echo "Usage: $0 [json|config|full|all]"
        echo ""
        echo "Commands:"
        echo "  json     - Run JSON extraction unit test (fast, no APIs)"
        echo "  config   - Run config parsing test with real Kimi"
        echo "  full     - Run full end-to-end test with real APIs"
        echo "  all      - Run all tests (default)"
        exit 1
        ;;
esac
