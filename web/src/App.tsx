import { useGnssData } from "./hooks/useGnssData";
import { FileLoader } from "./components/FileLoader";
import type { ProcessingResult } from "./types/gnss";
import "./App.css";

export default function App() {
  const { state, processFile } = useGnssData();

  return (
    <div className="app">
      <header className="app-header">
        <h1>trajix</h1>
      </header>

      <main className="app-main">
        {state.status !== "done" ? (
          <FileLoader state={state} onFile={processFile} />
        ) : (
          <ResultSummary result={state.result} />
        )}
      </main>
    </div>
  );
}

function ResultSummary({ result }: { result: ProcessingResult }) {
  const c = result.record_counts;
  return (
    <div className="result-summary">
      <h2>Parsed {result.lines_parsed.toLocaleString()} lines</h2>
      {result.header && (
        <p className="device-info">
          {result.header.manufacturer} {result.header.model} (
          {result.header.version})
        </p>
      )}
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
      <p>
        {result.fixes.length.toLocaleString()} fixes,{" "}
        {result.status_epochs.length.toLocaleString()} status epochs,{" "}
        {result.fix_epochs.length.toLocaleString()} fix epochs,{" "}
        {result.dr_trajectory.length.toLocaleString()} DR points
      </p>
    </div>
  );
}
