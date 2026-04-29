export type QueueRunStatus = "idle" | "running" | "paused" | "finished" | "failed";

export interface QueueRun {
  id: string;
  status: QueueRunStatus;
  pause_on_error: boolean;
  created_at: string;
  updated_at: string;
  started_at?: string;
  finished_at?: string;
}
