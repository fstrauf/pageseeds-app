import type { QueueItem } from "./QueueItem";
import type { QueueRun } from "./QueueRun";

export interface QueueSnapshot {
  run?: QueueRun;
  items: QueueItem[];
}
