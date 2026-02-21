import { useCallback, useEffect, useRef, useState } from "react";
import * as Cesium from "cesium";
import "./PlaybackControls.css";

const SPEED_OPTIONS = [1, 10, 50, 100, 500] as const;

interface PlaybackControlsProps {
  viewer: Cesium.Viewer | null;
}

export function PlaybackControls({ viewer }: PlaybackControlsProps) {
  const [isPlaying, setIsPlaying] = useState(false);
  const [speed, setSpeed] = useState(50);
  const [isFollowing, setIsFollowing] = useState(false);
  const [progress, setProgress] = useState(0);
  const [elapsedStr, setElapsedStr] = useState("00:00:00");
  const [totalStr, setTotalStr] = useState("00:00:00");

  const rafRef = useRef<number>(0);
  const markerRef = useRef<Cesium.Entity | null>(null);

  // Find the animated marker entity (the one with a SampledPositionProperty)
  const findMarker = useCallback((): Cesium.Entity | null => {
    if (!viewer) return null;
    if (markerRef.current) return markerRef.current;

    const entities = viewer.entities.values;
    for (const entity of entities) {
      if (entity.position instanceof Cesium.SampledPositionProperty) {
        markerRef.current = entity;
        return entity;
      }
    }
    return null;
  }, [viewer]);

  // Compute total duration string on mount / viewer change
  useEffect(() => {
    if (!viewer) return;
    const clock = viewer.clock;
    const totalSec = Cesium.JulianDate.secondsDifference(
      clock.stopTime,
      clock.startTime,
    );
    setTotalStr(formatDuration(totalSec));
  }, [viewer]);

  // Animation frame loop — reads clock and updates UI at ~10fps
  useEffect(() => {
    if (!viewer) return;

    let lastUpdate = 0;
    const tick = (now: number) => {
      rafRef.current = requestAnimationFrame(tick);

      // Throttle UI updates to ~10fps
      if (now - lastUpdate < 100) return;
      lastUpdate = now;

      const clock = viewer.clock;
      const totalSec = Cesium.JulianDate.secondsDifference(
        clock.stopTime,
        clock.startTime,
      );
      if (totalSec <= 0) return;

      const elapsedSec = Cesium.JulianDate.secondsDifference(
        clock.currentTime,
        clock.startTime,
      );
      const ratio = Math.max(0, Math.min(1, elapsedSec / totalSec));

      setProgress(ratio);
      setElapsedStr(formatDuration(elapsedSec));

      // Sync React state with clock (e.g. if clock auto-paused at end)
      setIsPlaying(clock.shouldAnimate);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [viewer]);

  // Reset marker ref when entities change
  useEffect(() => {
    markerRef.current = null;
  }, [viewer]);

  const togglePlay = useCallback(() => {
    if (!viewer) return;
    const clock = viewer.clock;

    // If at the end, restart from beginning
    if (
      !clock.shouldAnimate &&
      Cesium.JulianDate.compare(clock.currentTime, clock.stopTime) >= 0
    ) {
      clock.currentTime = clock.startTime.clone();
    }

    clock.shouldAnimate = !clock.shouldAnimate;
    setIsPlaying(clock.shouldAnimate);
  }, [viewer]);

  const changeSpeed = useCallback(
    (newSpeed: number) => {
      if (!viewer) return;
      viewer.clock.multiplier = newSpeed;
      setSpeed(newSpeed);
    },
    [viewer],
  );

  const handleSeek = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!viewer) return;
      const ratio = parseFloat(e.target.value);
      const clock = viewer.clock;
      const totalSec = Cesium.JulianDate.secondsDifference(
        clock.stopTime,
        clock.startTime,
      );
      const newTime = Cesium.JulianDate.addSeconds(
        clock.startTime,
        ratio * totalSec,
        new Cesium.JulianDate(),
      );
      clock.currentTime = newTime;
    },
    [viewer],
  );

  const toggleFollow = useCallback(() => {
    if (!viewer) return;
    const marker = findMarker();

    if (isFollowing) {
      // Unfollow
      viewer.trackedEntity = undefined;
      setIsFollowing(false);
    } else if (marker) {
      // Follow marker
      viewer.trackedEntity = marker;
      setIsFollowing(true);
    }
  }, [viewer, isFollowing, findMarker]);

  if (!viewer) return null;

  return (
    <div className="playback-controls">
      <button
        className="playback-btn play-btn"
        onClick={togglePlay}
        title={isPlaying ? "Pause" : "Play"}
      >
        {isPlaying ? "\u23F8" : "\u25B6"}
      </button>

      <select
        className="playback-speed"
        value={speed}
        onChange={(e) => changeSpeed(Number(e.target.value))}
        title="Playback speed"
      >
        {SPEED_OPTIONS.map((s) => (
          <option key={s} value={s}>
            {s}x
          </option>
        ))}
      </select>

      <input
        className="playback-seek"
        type="range"
        min={0}
        max={1}
        step={0.0001}
        value={progress}
        onChange={handleSeek}
        title="Seek"
      />

      <span className="playback-time">
        {elapsedStr} / {totalStr}
      </span>

      <button
        className={`playback-btn follow-btn ${isFollowing ? "active" : ""}`}
        onClick={toggleFollow}
        title={isFollowing ? "Free camera" : "Follow marker"}
      >
        {isFollowing ? "\u{1F3AF}" : "\u{1F4CD}"}
      </button>
    </div>
  );
}

function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${pad2(h)}:${pad2(m)}:${pad2(sec)}`;
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}
