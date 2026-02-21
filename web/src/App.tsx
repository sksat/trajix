import { useCallback, useState } from "react";
import type * as Cesium from "cesium";
import { useGnssData } from "./hooks/useGnssData";
import { FileLoader } from "./components/FileLoader";
import { CesiumMap } from "./components/CesiumMap";
import { PlaybackControls } from "./components/PlaybackControls";
import type { ProcessingResult } from "./types/gnss";
import "./App.css";

export default function App() {
  const { state, processFile } = useGnssData();
  const [showNlp, setShowNlp] = useState(false);
  const [viewer, setViewer] = useState<Cesium.Viewer | null>(null);

  const handleViewerReady = useCallback((v: Cesium.Viewer) => {
    setViewer(v);
  }, []);

  return (
    <div className="app">
      <header className="app-header">
        <h1>trajix</h1>
        {state.status === "done" && state.result.header && (
          <span className="device-badge">
            {state.result.header.manufacturer} {state.result.header.model}
          </span>
        )}
      </header>

      {state.status !== "done" ? (
        <main className="app-main center">
          <FileLoader state={state} onFile={processFile} />
        </main>
      ) : (
        <div className="app-content">
          <div className="map-panel">
            <CesiumMap
              result={state.result}
              showNlp={showNlp}
              onViewerReady={handleViewerReady}
            />
            <PlaybackControls viewer={viewer} />
          </div>
          <aside className="sidebar">
            <ResultSummary result={state.result} />
            <div className="layer-controls">
              <h3>Layers</h3>
              <label className="toggle-label">
                <input
                  type="checkbox"
                  checked={showNlp}
                  onChange={(e) => setShowNlp(e.target.checked)}
                />
                Show NLP fixes
              </label>
              <FixQualitySummary result={state.result} />
            </div>
          </aside>
        </div>
      )}
    </div>
  );
}

function ResultSummary({ result }: { result: ProcessingResult }) {
  const c = result.record_counts;
  return (
    <div className="result-summary">
      <h3>Parsed {result.lines_parsed.toLocaleString()} lines</h3>
      <table>
        <tbody>
          <tr>
            <td>Fix</td>
            <td>{c.fix.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Status</td>
            <td>{c.status.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Raw</td>
            <td>{c.raw.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Accel</td>
            <td>{c.uncal_accel.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Gyro</td>
            <td>{c.uncal_gyro.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Mag</td>
            <td>{c.uncal_mag.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Orientation</td>
            <td>{c.orientation.toLocaleString()}</td>
          </tr>
          <tr>
            <td>GameRotation</td>
            <td>{c.game_rotation.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Skipped</td>
            <td>{c.skipped.toLocaleString()}</td>
          </tr>
          <tr>
            <td>Errors</td>
            <td>{c.errors.toLocaleString()}</td>
          </tr>
        </tbody>
      </table>
      <p className="summary-counts">
        {result.fixes.length.toLocaleString()} fixes,{" "}
        {result.status_epochs.length.toLocaleString()} status epochs,{" "}
        {result.dr_trajectory.length.toLocaleString()} DR points
      </p>
    </div>
  );
}

function FixQualitySummary({ result }: { result: ProcessingResult }) {
  const primary = result.fix_qualities.filter((q) => q === "Primary").length;
  const fallback = result.fix_qualities.filter(
    (q) => q === "GapFallback",
  ).length;
  const rejected = result.fix_qualities.filter(
    (q) => q === "Rejected",
  ).length;

  return (
    <div className="fix-quality-summary">
      <table>
        <tbody>
          <tr>
            <td>Primary (GPS+FLP)</td>
            <td>{primary.toLocaleString()}</td>
          </tr>
          <tr>
            <td>NLP gap fallback</td>
            <td>{fallback.toLocaleString()}</td>
          </tr>
          <tr>
            <td>NLP rejected</td>
            <td>{rejected.toLocaleString()}</td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}
