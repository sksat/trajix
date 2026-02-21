import { useCallback, useState, type DragEvent } from "react";
import type { GnssDataState } from "../hooks/useGnssData";

interface FileLoaderProps {
  state: GnssDataState;
  onFile: (file: File) => void;
}

export function FileLoader({ state, onFile }: FileLoaderProps) {
  const [dragOver, setDragOver] = useState(false);

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  }, []);

  const handleDragLeave = useCallback(() => {
    setDragOver(false);
  }, []);

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      const file = e.dataTransfer.files[0];
      if (file) onFile(file);
    },
    [onFile],
  );

  const handleClick = useCallback(() => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".txt,.csv,.log";
    input.onchange = () => {
      const file = input.files?.[0];
      if (file) onFile(file);
    };
    input.click();
  }, [onFile]);

  if (state.status === "loading") {
    const pct = Math.round(state.progress * 100);
    return (
      <div className="file-loader">
        <div className="progress-container">
          <div className="progress-bar" style={{ width: `${pct}%` }} />
        </div>
        <p className="progress-text">
          Parsing... {pct}% ({state.linesParsed.toLocaleString()} lines)
        </p>
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className="file-loader error" onClick={handleClick}>
        <p>Error: {state.message}</p>
        <p className="hint">Click or drop a file to retry</p>
      </div>
    );
  }

  return (
    <div
      className={`file-loader drop-zone ${dragOver ? "drag-over" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onClick={handleClick}
    >
      <p className="drop-label">Drop GNSS Logger .txt file here</p>
      <p className="hint">or click to browse</p>
    </div>
  );
}
