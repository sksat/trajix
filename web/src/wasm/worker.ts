import init, { GnssLogProcessor } from "trajix-wasm";
import type { WorkerRequest, WorkerResponse } from "./protocol";

let processor: GnssLogProcessor | null = null;
let totalBytes = 0;

function post(msg: WorkerResponse) {
  self.postMessage(msg);
}

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;

  try {
    switch (msg.type) {
      case "start": {
        await init();
        processor = new GnssLogProcessor();
        totalBytes = msg.totalBytes;
        post({ type: "ready" });
        break;
      }

      case "chunk": {
        if (!processor) throw new Error("Processor not initialized");
        processor.feed(msg.data);
        const ratio = processor.progress(BigInt(totalBytes));
        const linesParsed = Number(processor.lines_parsed());
        post({ type: "progress", ratio, linesParsed });
        break;
      }

      case "finalize": {
        if (!processor) throw new Error("Processor not initialized");
        const data = processor.finalize();
        processor = null;
        post({ type: "result", data });
        break;
      }
    }
  } catch (err) {
    post({ type: "error", message: String(err) });
  }
};
