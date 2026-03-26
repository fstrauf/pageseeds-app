#!/bin/bash
#
# Queue System Test Runner
#
# Runs various levels of tests for the queue and logging system
#
# Usage:
#   ./scripts/test-queue-system.sh [level]
#
# Levels:
#   unit      - Fast unit tests (no external dependencies)
#   int       - Integration tests (SQLite only, no APIs)
#   full      - Full E2E tests (requires real Kimi agent)
#   all       - Run all tests (default)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TAURI_DIR="$PROJECT_DIR/src-tauri"

cd "$TAURI_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

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

# Unit tests - fast, no external deps
run_unit_tests() {
    print_header "Running Unit Tests"
    
    # JSON extraction test
    echo "Testing JSON extraction..."
    cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output -- --nocapture
    
    print_success "Unit tests passed"
}

# Integration tests - SQLite only, no APIs
run_integration_tests() {
    print_header "Running Integration Tests"
    
    echo "Testing queue state management..."
    cargo test --test queue_integration_test test_queue_enqueue_and_state_management -- --nocapture
    
    echo "Testing batch log submission..."
    cargo test --test queue_integration_test test_batch_log_submission -- --nocapture
    
    echo "Testing log querying..."
    cargo test --test queue_integration_test test_log_querying -- --nocapture
    
    print_success "Integration tests passed"
}

# E2E tests - requires real APIs
run_e2e_tests() {
    print_header "Running E2E Tests (Requires Real APIs)"
    print_warning "These tests require:"
    print_warning "  - Kimi CLI to be installed and authenticated"
    print_warning "  - Internet connection for Reddit API"
    print_warning "  - ~60 seconds to complete"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo ""
    
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "Testing full queue flow with real execution..."
        cargo test --test queue_e2e_test test_full_queue_flow_with_real_execution -- --ignored --nocapture
        print_success "E2E tests passed"
    else
        print_warning "E2E tests skipped"
    fi
}

# Log persistence tests
run_log_tests() {
    print_header "Running Log System Tests"
    
    echo "Testing log persistence..."
    cargo test --test queue_e2e_test test_log_persistence -- --nocapture
    
    print_success "Log tests passed"
}

# Check prerequisites
check_prerequisites() {
    print_header "Checking Prerequisites"
    
    if ! command -v cargo &> /dev/null; then
        print_error "Cargo not found. Please install Rust."
        exit 1
    fi
    print_success "Cargo found"
    
    if ! command -v kimi &> /dev/null; then
        print_warning "Kimi CLI not found. E2E tests will be skipped."
    else
        print_success "Kimi CLI found"
    fi
}

# Main
LEVEL="${1:-all}"

check_prerequisites

case "$LEVEL" in
    unit)
        run_unit_tests
        ;;
    int|integration)
        run_integration_tests
        run_log_tests
        ;;
    e2e|full)
        run_e2e_tests
        ;;
    log|logs)
        run_log_tests
        ;;
    all)
        run_unit_tests
        run_integration_tests
        run_log_tests
        run_e2e_tests
        ;;
    *)
        echo "Usage: $0 [unit|int|e2e|log|all]"
        echo ""
        echo "Levels:"
        echo "  unit    - Fast unit tests only"
        echo "  int     - Integration tests (SQLite only)"
        echo "  e2e     - Full E2E tests (requires APIs)"
        echo "  log     - Log system tests only"
        echo "  all     - Run all tests (default)"
        exit 1
        ;;
esac

print_header "Test Summary"
print_success "All requested tests completed!"
