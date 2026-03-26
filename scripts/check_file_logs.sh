#!/bin/bash
# Check file-based logs for queue debugging

echo "=========================================="
echo "File-based Log Analysis"
echo "=========================================="
echo ""

LOG_DIR="$HOME/Library/Logs/com.pageseeds.app"

if [ ! -d "$LOG_DIR" ]; then
    echo "Log directory not found: $LOG_DIR"
    exit 1
fi

echo "Log directory: $LOG_DIR"
echo ""

# Find the most recent log file
LATEST_LOG=$(ls -t "$LOG_DIR"/PageSeeds*.log 2>/dev/null | head -1)

if [ -z "$LATEST_LOG" ]; then
    echo "No log files found"
    exit 1
fi

echo "Latest log file: $LATEST_LOG"
echo ""

echo "1. Queue execution flow (last 100 lines with queue/executor):"
echo "----------------------------------------"
grep -E "(execute_queue|queue_internal|task-started|task-completed)" "$LATEST_LOG" | tail -50

echo ""
echo "2. All ERROR logs:"
echo "----------------------------------------"
grep "ERROR" "$LATEST_LOG" | tail -20

echo ""
echo "3. Last 30 log lines:"
echo "----------------------------------------"
tail -30 "$LATEST_LOG"

echo ""
echo "=========================================="
echo "Full log file: $LATEST_LOG"
echo "=========================================="
