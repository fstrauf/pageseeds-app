export type QueueItemStatus = "pending" | "running" | "completed" | "failed" | "skipped";

export interface QueueItem {
  run_id: string;
  position: number;
  task_id: string;
  project_id: string;
  status: QueueItemStatus;
  error?: string;
  result_json?: string;
  created_at: string;
  updated_at: string;
  title?: string;
  task_type?: string;
  project_name?: string;
}
