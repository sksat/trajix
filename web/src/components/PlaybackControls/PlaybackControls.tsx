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

    if (isFollowing) {
      setIsFollowing(false);
    } else if (findMarker()) {
      setIsFollowing(true);
    }
  }, [viewer, isFollowing, findMarker]);

  // Smoothed camera follow via preRender listener.
  // Instead of trackedEntity (which rigidly follows entity position and
  // transmits GPS altitude jitter to the camera), we lerp toward the
  // marker's position in Cartographic space with heavy vertical smoothing.
  useEffect(() => {
    if (!viewer || !isFollowing) return;
    const marker = findMarker();
    if (!marker) return;

    let smoothed: Cesium.Cartographic | null = null;

    // Lerp factors per frame (~60fps).
    // Horizontal: tau ~0.3s → responsive tracking of lateral movement.
    // Vertical:   tau ~3s   → heavily dampens altitude jitter.
    const H_LERP = 0.08;
    const V_LERP = 0.015;
    // If the marker jumps further than this (e.g. seek), snap immediately
    const SNAP_RAD = 0.002; // ~200m in radians at equator

    // Initial camera geometry matching viewFrom(0, -200, 300):
    //   range ≈ 360m, pitch ≈ -56°, heading = 0 (looking north)
    // After the first frame, we read the camera's current HPR so that
    // user drag/scroll adjustments are preserved across frames.
    let heading = 0;
    let pitch = Cesium.Math.toRadians(-56);
    let range = 360;
    let initialized = false;

    // --- Auto-heading toward travel direction ---
    // Adaptive EMA on bearing: short window on turns, long on straights.
    let prevSample: { lon: number; lat: number } | null = null;
    const POS_SAMPLE_RAD = 0.00005; // ~5m min distance between samples
    // Exponentially smoothed bearing (sin/cos for circular averaging)
    let bearingSin = 0;
    let bearingCos = 0;
    let travelBearing = 0;
    let hasBearing = false;
    // Adaptive alpha: responsive on turns, stable on straights
    const ALPHA_MIN = 0.05; // ~20 samples ≈ 100m effective window
    const ALPHA_MAX = 0.4; // ~2.5 samples ≈ 12m effective window
    const TURN_THRESHOLD = Math.PI / 4; // 45° → full responsiveness
    // Auto-heading lerp speed (per frame ~60fps), tau ~4s
    const HEADING_LERP = 0.015;
    // User's preferred offset from travel direction (updated by drag)
    let headingOffset = Cesium.Math.toRadians(10);
    let lastSetHeading = 0;
    // Pitch range: no auto-heading near top-down, full at moderate angles
    const PITCH_FULL = Cesium.Math.toRadians(-60);
    const PITCH_ZERO = Cesium.Math.toRadians(-80);

    const onPreRender = () => {
      const pos = marker.position?.getValue(viewer.clock.currentTime);
      if (!pos) return;

      const target = Cesium.Cartographic.fromCartesian(pos);

      if (!smoothed) {
        smoothed = target.clone();
      } else {
        const dLon = target.longitude - smoothed.longitude;
        const dLat = target.latitude - smoothed.latitude;
        const dH = target.height - smoothed.height;

        // Snap on large jumps (seek / track restart)
        if (Math.abs(dLon) > SNAP_RAD || Math.abs(dLat) > SNAP_RAD) {
          smoothed = target.clone();
          prevSample = null;
          hasBearing = false;
        } else {
          smoothed.longitude += dLon * H_LERP;
          smoothed.latitude += dLat * H_LERP;
          smoothed.height += dH * V_LERP;
        }
      }

      // Sample smoothed position & compute adaptive bearing via EMA
      const moved =
        !prevSample ||
        Math.abs(smoothed.longitude - prevSample.lon) > POS_SAMPLE_RAD ||
        Math.abs(smoothed.latitude - prevSample.lat) > POS_SAMPLE_RAD;
      if (moved) {
        if (prevSample) {
          const dLon = smoothed.longitude - prevSample.lon;
          // Instantaneous bearing from previous sample
          const inst = Math.atan2(
            Math.sin(dLon) * Math.cos(smoothed.latitude),
            Math.cos(prevSample.lat) * Math.sin(smoothed.latitude) -
              Math.sin(prevSample.lat) *
                Math.cos(smoothed.latitude) *
                Math.cos(dLon),
          );
          if (!hasBearing) {
            bearingSin = Math.sin(inst);
            bearingCos = Math.cos(inst);
            hasBearing = true;
          } else {
            // Adaptive alpha: large on sharp turns, small on straights
            const cur = Math.atan2(bearingSin, bearingCos);
            const delta = Math.abs(
              Math.atan2(Math.sin(inst - cur), Math.cos(inst - cur)),
            );
            const alpha =
              ALPHA_MIN +
              (ALPHA_MAX - ALPHA_MIN) * Math.min(1, delta / TURN_THRESHOLD);
            bearingSin = bearingSin * (1 - alpha) + Math.sin(inst) * alpha;
            bearingCos = bearingCos * (1 - alpha) + Math.cos(inst) * alpha;
          }
          travelBearing = Math.atan2(bearingSin, bearingCos);
        }
        prevSample = { lon: smoothed.longitude, lat: smoothed.latitude };
      }

      // Read user-adjusted HPR before overwriting (preserves drag/zoom)
      if (initialized) {
        const cameraHeading = viewer.camera.heading;
        // Detect user heading drag → update offset from travel direction
        const userDelta = Math.atan2(
          Math.sin(cameraHeading - lastSetHeading),
          Math.cos(cameraHeading - lastSetHeading),
        );
        if (Math.abs(userDelta) > 0.003) {
          headingOffset = Math.atan2(
            Math.sin(headingOffset + userDelta),
            Math.cos(headingOffset + userDelta),
          );
        }
        heading = cameraHeading;
        pitch = viewer.camera.pitch;
        const camPos = viewer.camera.positionWC;
        const center = Cesium.Cartesian3.fromRadians(
          smoothed.longitude,
          smoothed.latitude,
          smoothed.height,
        );
        range = Cesium.Cartesian3.distance(camPos, center);
      }

      // Auto-rotate heading toward travel direction (when not top-down)
      if (hasBearing && initialized) {
        // Pitch-dependent blend: 0 near top-down, 1 at moderate pitch
        const t = Math.max(
          0,
          Math.min(1, (pitch - PITCH_ZERO) / (PITCH_FULL - PITCH_ZERO)),
        );
        const desired = travelBearing + headingOffset;
        // Shortest angle difference via atan2
        const diff = Math.atan2(
          Math.sin(desired - heading),
          Math.cos(desired - heading),
        );
        heading += diff * HEADING_LERP * t;
      }

      // Prevent camera from clipping into terrain: if camera would be
      // below terrain + margin, pull pitch toward nadir (more top-down).
      const center = Cesium.Cartesian3.fromRadians(
        smoothed.longitude,
        smoothed.latitude,
        smoothed.height,
      );
      const camOffset = new Cesium.HeadingPitchRange(heading, pitch, range);
      // Compute would-be camera position
      viewer.camera.lookAt(center, camOffset);
      const camCarto = Cesium.Cartographic.fromCartesian(
        viewer.camera.positionWC,
      );
      if (camCarto) {
        const terrainH = viewer.scene.globe.getHeight(camCarto);
        if (terrainH !== undefined) {
          const MIN_ALT = 50; // minimum camera altitude above terrain (m)
          const camAlt = camCarto.height - terrainH;
          if (camAlt < MIN_ALT) {
            // Pull pitch toward nadir to lift camera above terrain
            const PITCH_ADJUST = 0.03; // per-frame adjustment speed
            const deficit = (MIN_ALT - camAlt) / MIN_ALT; // 0..1+
            pitch = pitch - PITCH_ADJUST * Math.min(1, deficit);
            // Clamp pitch to not go past straight-down
            pitch = Math.max(pitch, Cesium.Math.toRadians(-89));
          }
        }
      }

      viewer.camera.lookAt(
        center,
        new Cesium.HeadingPitchRange(heading, pitch, range),
      );
      lastSetHeading = heading;
      initialized = true;
    };

    viewer.scene.preRender.addEventListener(onPreRender);

    return () => {
      viewer.scene.preRender.removeEventListener(onPreRender);
      // Unlock camera so user can freely pan/rotate
      viewer.camera.lookAtTransform(Cesium.Matrix4.IDENTITY);
    };
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
