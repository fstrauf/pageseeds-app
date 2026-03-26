/**
 * Log Viewer Component
 * 
 * Allows viewing and searching through application logs stored in the database.
 * Useful for debugging and agentic analysis.
 */

import { useState, useEffect, useCallback } from 'react';
import { queryLogs, getLogStats, exportLogs, clearOldLogs, flushLogs } from '@/lib/logging';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ScrollArea } from '@/components/ui/scroll-area';
import { RefreshCw, Download, Trash2, Search } from 'lucide-react';

interface LogEntry {
  id?: number;
  timestamp: string;
  level: string;
  source: string;
  component: string;
  message: string;
  metadata?: Record<string, unknown>;
  session_id: string;
}

export function LogViewer() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [stats, setStats] = useState<{
    total_count: number;
    error_count: number;
    warn_count: number;
    info_count: number;
    debug_count: number;
    frontend_count: number;
    backend_count: number;
    agent_count: number;
  } | null>(null);
  const [loading, setLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [levelFilter, setLevelFilter] = useState<string>('all');
  const [sourceFilter, setSourceFilter] = useState<string>('all');
  const [limit, setLimit] = useState(100);

  const fetchLogs = useCallback(async () => {
    setLoading(true);
    try {
      // Flush any pending frontend logs first
      await flushLogs();
      
      const filters: {
        level?: string;
        source?: string;
        searchQuery?: string;
      } = {};
      
      if (levelFilter !== 'all') filters.level = levelFilter;
      if (sourceFilter !== 'all') filters.source = sourceFilter;
      if (searchQuery.trim()) filters.searchQuery = searchQuery.trim();
      
      const results = await queryLogs(filters, limit, 0);
      setLogs(results);
      
      // Also fetch stats
      const statsResult = await getLogStats();
      setStats(statsResult);
    } catch (error) {
      console.error('Failed to fetch logs:', error);
    } finally {
      setLoading(false);
    }
  }, [levelFilter, sourceFilter, searchQuery, limit]);

  useEffect(() => {
    fetchLogs();
    
    // Auto-refresh every 10 seconds
    const interval = setInterval(fetchLogs, 10000);
    return () => clearInterval(interval);
  }, [fetchLogs]);

  const handleExport = async () => {
    const json = await exportLogs({
      level: levelFilter !== 'all' ? levelFilter : undefined,
      source: sourceFilter !== 'all' ? sourceFilter : undefined,
      searchQuery: searchQuery.trim() || undefined,
    });
    
    if (json) {
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `pageseeds-logs-${new Date().toISOString().split('T')[0]}.json`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    }
  };

  const handleClearOld = async () => {
    if (confirm('Clear logs older than 7 days?')) {
      const count = await clearOldLogs(7);
      alert(`Cleared ${count} old log entries`);
      fetchLogs();
    }
  };

  const getLevelColor = (level: string) => {
    switch (level.toLowerCase()) {
      case 'error': return 'bg-red-100 text-red-800 border-red-200';
      case 'warn': return 'bg-amber-100 text-amber-800 border-amber-200';
      case 'info': return 'bg-blue-100 text-blue-800 border-blue-200';
      case 'debug': return 'bg-gray-100 text-gray-800 border-gray-200';
      default: return 'bg-gray-100 text-gray-800 border-gray-200';
    }
  };

  const getSourceColor = (source: string) => {
    switch (source.toLowerCase()) {
      case 'frontend': return 'text-purple-600';
      case 'backend': return 'text-emerald-600';
      case 'agent': return 'text-cyan-600';
      default: return 'text-gray-600';
    }
  };

  const formatTimestamp = (ts: string) => {
    const date = new Date(ts);
    return date.toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  };

  return (
    <div className="space-y-4">
      {/* Stats */}
      {stats && (
        <div className="grid grid-cols-4 gap-2 text-xs">
          <div className="bg-gray-50 p-2 rounded border">
            <div className="text-gray-500">Total</div>
            <div className="font-semibold">{stats.total_count.toLocaleString()}</div>
          </div>
          <div className="bg-red-50 p-2 rounded border border-red-100">
            <div className="text-red-600">Errors</div>
            <div className="font-semibold text-red-700">{stats.error_count.toLocaleString()}</div>
          </div>
          <div className="bg-amber-50 p-2 rounded border border-amber-100">
            <div className="text-amber-600">Warnings</div>
            <div className="font-semibold text-amber-700">{stats.warn_count.toLocaleString()}</div>
          </div>
          <div className="bg-blue-50 p-2 rounded border border-blue-100">
            <div className="text-blue-600">Info</div>
            <div className="font-semibold text-blue-700">{stats.info_count.toLocaleString()}</div>
          </div>
        </div>
      )}

      {/* Filters */}
      <div className="flex gap-2 items-center">
        <div className="relative flex-1">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-4 w-4 text-gray-400" />
          <Input
            placeholder="Search logs..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-8 h-8 text-sm"
            onKeyDown={(e) => e.key === 'Enter' && fetchLogs()}
          />
        </div>
        
        <Select value={levelFilter} onValueChange={setLevelFilter}>
          <SelectTrigger className="w-28 h-8 text-xs">
            <SelectValue placeholder="Level" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Levels</SelectItem>
            <SelectItem value="error">Error</SelectItem>
            <SelectItem value="warn">Warn</SelectItem>
            <SelectItem value="info">Info</SelectItem>
            <SelectItem value="debug">Debug</SelectItem>
          </SelectContent>
        </Select>

        <Select value={sourceFilter} onValueChange={setSourceFilter}>
          <SelectTrigger className="w-28 h-8 text-xs">
            <SelectValue placeholder="Source" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Sources</SelectItem>
            <SelectItem value="frontend">Frontend</SelectItem>
            <SelectItem value="backend">Backend</SelectItem>
            <SelectItem value="agent">Agent</SelectItem>
          </SelectContent>
        </Select>

        <Select value={limit.toString()} onValueChange={(v) => setLimit(parseInt(v))}>
          <SelectTrigger className="w-24 h-8 text-xs">
            <SelectValue placeholder="Limit" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="50">50</SelectItem>
            <SelectItem value="100">100</SelectItem>
            <SelectItem value="250">250</SelectItem>
            <SelectItem value="500">500</SelectItem>
          </SelectContent>
        </Select>

        <Button
          variant="ghost"
          size="sm"
          onClick={fetchLogs}
          disabled={loading}
          className="h-8 px-2"
        >
          <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={handleExport}
          className="h-8 px-2"
          title="Export logs"
        >
          <Download className="h-4 w-4" />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={handleClearOld}
          className="h-8 px-2 text-red-600 hover:text-red-700"
          title="Clear old logs"
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>

      {/* Logs List */}
      <ScrollArea className="h-96 border rounded-md">
        <div className="space-y-1 p-2">
          {logs.length === 0 ? (
            <div className="text-center text-gray-400 py-8 text-sm">
              No logs found
            </div>
          ) : (
            logs.map((log) => (
              <div
                key={log.id || `${log.timestamp}-${Math.random()}`}
                className="text-xs font-mono border-b border-gray-100 pb-1 last:border-0"
              >
                <div className="flex items-start gap-2">
                  <span className="text-gray-400 whitespace-nowrap">
                    {formatTimestamp(log.timestamp)}
                  </span>
                  <Badge 
                    variant="outline" 
                    className={`text-[10px] px-1 h-4 ${getLevelColor(log.level)}`}
                  >
                    {log.level.toUpperCase()}
                  </Badge>
                  <span className={`text-[10px] ${getSourceColor(log.source)}`}>
                    {log.source}
                  </span>
                  <span className="text-gray-500 text-[10px]">
                    {log.component}
                  </span>
                </div>
                <div className="pl-16 text-gray-700 mt-0.5">
                  {log.message}
                </div>
                {log.metadata && Object.keys(log.metadata).length > 0 && (
                  <div className="pl-16 text-[10px] text-gray-400 mt-0.5">
                    {JSON.stringify(log.metadata).slice(0, 200)}
                    {JSON.stringify(log.metadata).length > 200 && '...'}
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      </ScrollArea>

      <div className="text-xs text-gray-400 text-center">
        Showing {logs.length} entries • Auto-refreshes every 10s
      </div>
    </div>
  );
}
