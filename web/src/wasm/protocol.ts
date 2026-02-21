import type { ProcessingResult } from "../types/gnss";

/** Messages sent from main thread to worker. */
export type WorkerRequest =
  | { type: "start"; totalBytes: number }
  | { type: "chunk"; data: Uint8Array }
  | { type: "finalize" };

/** Messages sent from worker to main thread. */
export type WorkerResponse =
  | { type: "ready" }
  | { type: "progress"; ratio: number; linesParsed: number }
  | { type: "result"; data: ProcessingResult }
  | { type: "error"; message: string };
