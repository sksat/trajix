import { useCallback, useState } from "react";
import type * as Cesium from "cesium";
import { useGnssData } from "./hooks/useGnssData";
import { useDuckDB } from "./hooks/useDuckDB";
import { useAnimationTime } from "./hooks/useAnimationTime";
import { FileLoader } from "./components/FileLoader";
import { CesiumMap, GSI_IMAGERY } from "./components/CesiumMap";
import type { GsiImageryKey } from "./components/CesiumMap";
import { PlaybackControls } from "./components/PlaybackControls";
import { TimeSeriesPanel } from "./components/TimeSeries";
import { SkyPlot } from "./components/SkyPlot";
import type { ProcessingResult } from "./types/gnss";
import "./App.css";

function CollapsibleSection({
  title,
  defaultOpen = true,
  children,
}: {
  title: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={`collapsible-section ${open ? "open" : ""}`}>
      <button className="collapsible-header" onClick={() => setOpen(!open)}>
        <span className="collapsible-chevron">
          {open ? "\u25BE" : "\u25B8"}
        </span>
        {title}
      </button>
      {open && <div className="collapsible-body">{children}</div>}
    </div>
  );
}

export default function App() {
  const { state, processFile } = useGnssData();
  const duckdb = useDuckDB();
  const [showNlp, setShowNlp] = useState(false);
  const [imagery, setImagery] = useState<GsiImageryKey>("seamlessphoto");
  const [viewer, setViewer] = useState<Cesium.Viewer | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);

  const { currentTimeMs, seekTo } = useAnimationTime(viewer);

  const handleViewerReady = useCallback((v: Cesium.Viewer) => {
    setViewer(v);
  }, []);

  // Trigger DuckDB loading when parsing completes
  const handleFile = useCallback(
    async (file: File) => {
      await processFile(file);
    },
    [processFile],
  );

  // Load DuckDB when result becomes available
  const result = state.status === "done" ? state.result : null;
  if (result && duckdb.status === "idle") {
    duckdb.loadResult(result);
  }

  return (
    <div className="app">
      <header className="app-header">
        <h1>trajix</h1>
        {result && result.header && (
          <span className="device-badge">
            {result.header.manufacturer} {result.header.model}
          </span>
        )}
        {duckdb.status === "loading" && (
          <span className="device-badge">DuckDB loading...</span>
        )}
        {duckdb.status === "ready" && (
          <span className="device-badge status-ready">DuckDB ready</span>
        )}
      </header>

      {state.status !== "done" ? (
        <main className="app-main center">
          <FileLoader state={state} onFile={handleFile} />
        </main>
      ) : (
        <div className="app-content">
          <div className="upper-row">
            <div className="map-panel">
              <CesiumMap
                result={state.result}
                showNlp={showNlp}
                imagery={imagery}
                onViewerReady={handleViewerReady}
              />
              <PlaybackControls viewer={viewer} />
            </div>
            <aside className={`sidebar ${sidebarOpen ? "expanded" : ""}`}>
              <button
                className="sidebar-toggle"
                onClick={() => setSidebarOpen(!sidebarOpen)}
                aria-label={
                  sidebarOpen ? "Collapse sidebar" : "Expand sidebar"
                }
              >
                <span className="sidebar-toggle-handle" />
              </button>
              <CollapsibleSection title="Summary">
                <ResultSummary result={state.result} />
              </CollapsibleSection>
              <CollapsibleSection title="Sky Plot">
                <SkyPlot
                  snapshots={state.result.satellite_snapshots ?? []}
                  currentTimeMs={currentTimeMs}
                />
              </CollapsibleSection>
              <CollapsibleSection title="Layers">
                <div className="layer-controls">
                  <label className="toggle-label">
                    地図:
                    <select
                      value={imagery}
                      onChange={(e) =>
                        setImagery(e.target.value as GsiImageryKey)
                      }
                    >
                      {Object.entries(GSI_IMAGERY).map(([key, { label }]) => (
                        <option key={key} value={key}>
                          {label}
                        </option>
                      ))}
                    </select>
                  </label>
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
              </CollapsibleSection>
            </aside>
          </div>
          <TimeSeriesPanel
            statusEpochs={state.result.status_epochs}
            fixEpochs={state.result.fix_epochs}
            currentTimeMs={currentTimeMs}
            onSeek={seekTo}
          />
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
