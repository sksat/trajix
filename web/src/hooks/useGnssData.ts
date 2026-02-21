import { useCallback, useRef, useState } from "react";
import type { ProcessingResult } from "../types/gnss";
import type { WorkerResponse } from "../wasm/protocol";

export type GnssDataState =
  | { status: "idle" }
  | { status: "loading"; progress: number; linesParsed: number }
  | { status: "done"; result: ProcessingResult }
  | { status: "error"; message: string };

const CHUNK_SIZE = 4 * 1024 * 1024; // 4 MB

export function useGnssData() {
  const [state, setState] = useState<GnssDataState>({ status: "idle" });
  const workerRef = useRef<Worker | null>(null);

  const processFile = useCallback(async (file: File) => {
    // Clean up previous worker
    workerRef.current?.terminate();

    setState({ status: "loading", progress: 0, linesParsed: 0 });

    const worker = new Worker(
      new URL("../wasm/worker.ts", import.meta.url),
      { type: "module" },
    );
    workerRef.current = worker;

    worker.onmessage = async (e: MessageEvent<WorkerResponse>) => {
      const msg = e.data;

      switch (msg.type) {
        case "ready":
          // Worker initialized — start sending chunks
          await sendChunks(worker, file);
          break;

        case "progress":
          setState({
            status: "loading",
            progress: msg.ratio,
            linesParsed: msg.linesParsed,
          });
          break;

        case "result":
          // Debug: expose result to console
          (globalThis as any).__gnssResult = msg.data;
          setState({ status: "done", result: msg.data });
          worker.terminate();
          workerRef.current = null;
          break;

        case "error":
          setState({ status: "error", message: msg.message });
          worker.terminate();
          workerRef.current = null;
          break;
      }
    };

    worker.onerror = (err) => {
      setState({ status: "error", message: err.message });
      worker.terminate();
      workerRef.current = null;
    };

    // Initialize worker (triggers WASM init)
    worker.postMessage({ type: "start", totalBytes: file.size });
  }, []);

  return { state, processFile };
}

async function sendChunks(worker: Worker, file: File) {
  let offset = 0;
  while (offset < file.size) {
    const slice = file.slice(offset, offset + CHUNK_SIZE);
    const buffer = await slice.arrayBuffer();
    const chunk = new Uint8Array(buffer);
    worker.postMessage({ type: "chunk", data: chunk }, [chunk.buffer]);
    offset += CHUNK_SIZE;
  }
  worker.postMessage({ type: "finalize" });
}
