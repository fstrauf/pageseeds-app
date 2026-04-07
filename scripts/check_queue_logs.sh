#!/bin/bash
# Query logs for queue execution debugging
# Usage: ./check_queue_logs.sh

echo "=========================================="
echo "Queue Execution Log Analysis"
echo "=========================================="
echo ""

DB_PATH="$HOME/Library/Application Support/com.pageseeds.app/pageseeds.db"

if [ ! -f "$DB_PATH" ]; then
    echo "Database not found at: $DB_PATH"
    echo "Checking alternative locations..."
    
    # Try to find the database
    DB_PATH=$(find "$HOME/Library/Application Support" -name "pageseeds.db" 2>/dev/null | head -1)
    
    if [ -z "$DB_PATH" ]; then
        echo "Database not found. The app may not have run yet."
        exit 1
    fi
    
    echo "Found database at: $DB_PATH"
fi

echo "Database: $DB_PATH"
echo ""

# Check if sqlite3 is available
if ! command -v sqlite3 &> /dev/null; then
    echo "sqlite3 not found. Please install it to query logs."
    exit 1
fi

echo "1. Recent queue-related logs (last 50):"
echo "----------------------------------------"
sqlite3 "$DB_PATH" "SELECT 
    datetime(timestamp) as time,
    level,
    component,
    message
FROM app_logs
WHERE 
    component LIKE '%queue%' 
    OR message LIKE '%queue%'
    OR component LIKE '%executor%'
    OR message LIKE '%execute%'
ORDER BY timestamp DESC
LIMIT 50;"

echo ""
echo "2. All ERROR logs:"
echo "----------------------------------------"
sqlite3 "$DB_PATH" "SELECT 
    datetime(timestamp) as time,
    component,
    message
FROM app_logs
WHERE level = 'error'
ORDER BY timestamp DESC
LIMIT 20;"

echo ""
echo "3. Log stats by component:"
echo "----------------------------------------"
sqlite3 "$DB_PATH" "SELECT 
    component,
    COUNT(*) as count,
    MAX(datetime(timestamp)) as last_seen
FROM app_logs
WHERE timestamp > datetime('now', '-1 hour')
GROUP BY component
ORDER BY count DESC
LIMIT 20;"

echo ""
echo "4. Full trace for specific task (if known):"
echo "----------------------------------------"
# Try to find the most recent task execution
TASK_ID=$(sqlite3 "$DB_PATH" "SELECT 
    json_extract(metadata, '$.taskId') as task_id
FROM app_logs
WHERE metadata IS NOT NULL
    AND task_id IS NOT NULL
ORDER BY timestamp DESC
LIMIT 1;")

if [ -n "$TASK_ID" ]; then
    echo "Most recent task: $TASK_ID"
    sqlite3 "$DB_PATH" "SELECT 
        datetime(timestamp) as time,
        level,
        component,
        message
    FROM app_logs
    WHERE metadata LIKE '%$TASK_ID%'
        OR message LIKE '%$TASK_ID%'
    ORDER BY timestamp
    LIMIT 30;"
else
    echo "No task ID found in recent logs"
fi

echo ""
echo "=========================================="
echo "To see all logs, open LogViewer in app Settings"
echo "Or run: sqlite3 '$DB_PATH' 'SELECT * FROM app_logs ORDER BY timestamp DESC LIMIT 100;'"
echo "=========================================="
