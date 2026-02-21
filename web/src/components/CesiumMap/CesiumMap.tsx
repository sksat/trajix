import { useEffect, useRef } from "react";
import * as Cesium from "cesium";
import "cesium/Build/Cesium/Widgets/widgets.css";
import type { ProcessingResult, FixQuality } from "../../types/gnss";
import type { FixRecord } from "../../types/gnss";
import { accuracyToColor } from "../../utils/color";
import { createGsiTerrainProvider } from "../../utils/gsiTerrain";
import { filterAltitudeSpikes } from "../../utils/altitude";
import "./CesiumMap.css";

// Cesium ion token from env (optional — for PLATEAU 3D buildings)
const ION_TOKEN = import.meta.env.VITE_CESIUM_ION_TOKEN as string | undefined;

/// Gap threshold for breaking polylines (ms).
/// If consecutive Primary fixes are more than this apart, break the polyline.
const POLYLINE_GAP_MS = 3000;

/// Default playback speed multiplier (50x = ~8 min for 6.5h recording).
const DEFAULT_MULTIPLIER = 50;

interface CesiumMapProps {
  result: ProcessingResult;
  showNlp?: boolean;
  onViewerReady?: (viewer: Cesium.Viewer) => void;
}

export function CesiumMap({
  result,
  showNlp = false,
  onViewerReady,
}: CesiumMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<Cesium.Viewer | null>(null);

  // Initialize viewer once
  useEffect(() => {
    if (!containerRef.current) return;

    if (ION_TOKEN) {
      Cesium.Ion.defaultAccessToken = ION_TOKEN;
    }

    // GSI pale map tiles
    const gsiProvider = new Cesium.UrlTemplateImageryProvider({
      url: "https://cyberjapandata.gsi.go.jp/xyz/pale/{z}/{x}/{y}.png",
      credit: new Cesium.Credit(
        '<a href="https://maps.gsi.go.jp/development/ichiran.html">国土地理院</a>',
      ),
      minimumLevel: 2,
      maximumLevel: 18,
    });

    const viewer = new Cesium.Viewer(containerRef.current, {
      baseLayer: new Cesium.ImageryLayer(gsiProvider),
      terrainProvider: createGsiTerrainProvider(),
      animation: false,
      timeline: false,
      geocoder: false,
      homeButton: false,
      sceneModePicker: false,
      baseLayerPicker: false,
      navigationHelpButton: false,
      fullscreenButton: false,
      selectionIndicator: false,
      infoBox: false,
    });

    viewerRef.current = viewer;
    onViewerReady?.(viewer);

    return () => {
      viewer.destroy();
      viewerRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Render track + animation marker when result or NLP toggle changes.
  // Terrain sampling is async, so we use a cancellation flag.
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer || result.fixes.length === 0) return;

    let cancelled = false;

    const primaryFixes = result.fixes.filter(
      (_, i) => result.fix_qualities[i] === "Primary",
    );

    // Sample terrain heights, filter altitude spikes, then render
    clampBelowTerrain(viewer.terrainProvider, primaryFixes).then((heights) => {
      if (cancelled) return;

      // Filter altitude spikes (GPS/FLP interleaving + noise)
      const { filteredHeights, stats } = filterAltitudeSpikes(
        primaryFixes,
        heights,
      );
      if (stats.pointsReplaced > 0) {
        console.log(
          `Altitude filter: replaced ${stats.pointsReplaced} spike points (max deviation: ${stats.maxDeviation.toFixed(0)}m)`,
        );
      }

      viewer.entities.removeAll();

      addColoredTrack(viewer, primaryFixes, filteredHeights);

      if (primaryFixes.length >= 2) {
        setupAnimationMarker(viewer, primaryFixes, filteredHeights);
      }

      // Optional NLP layer
      if (showNlp) {
        const nlpFixes: { fix: FixRecord; quality: FixQuality }[] = [];
        for (let i = 0; i < result.fixes.length; i++) {
          const q = result.fix_qualities[i]!;
          if (q === "GapFallback" || q === "Rejected") {
            nlpFixes.push({ fix: result.fixes[i]!, quality: q });
          }
        }
        addNlpPoints(viewer, nlpFixes);
      }

      // Fly to entities with a tilted camera so terrain relief is visible
      viewer.flyTo(viewer.entities, {
        offset: new Cesium.HeadingPitchRange(
          0,
          Cesium.Math.toRadians(-45),
          0, // auto range
        ),
      });

      // Load PLATEAU 3D buildings if ion token available
      if (ION_TOKEN) {
        Cesium.Cesium3DTileset.fromIonAssetId(2602291).then((tileset) => {
          if (!cancelled) viewer.scene.primitives.add(tileset);
        });
      }
    });

    return () => {
      cancelled = true;
    };
  }, [result, showNlp]);

  return <div ref={containerRef} className="cesium-map-container" />;
}

// ────────────────────────────────────────────
// Terrain height adjustment
// ────────────────────────────────────────────

/**
 * Sample terrain heights at fix positions and return adjusted altitudes:
 *   max(gps_altitude, terrain_altitude)
 *
 * This ensures entities never sink below the terrain surface while
 * preserving above-ground altitude (e.g., ropeway / bridge sections).
 * GPS vertical accuracy is inherently lower than horizontal, so
 * occasional underground values are expected and corrected here.
 */
async function clampBelowTerrain(
  terrainProvider: Cesium.TerrainProvider,
  fixes: FixRecord[],
): Promise<number[]> {
  const cartographics = fixes.map((f) =>
    Cesium.Cartographic.fromDegrees(f.longitude_deg, f.latitude_deg),
  );

  try {
    await Cesium.sampleTerrainMostDetailed(terrainProvider, cartographics);
  } catch {
    // Terrain sampling failed — fall back to raw GPS altitude
    return fixes.map((f) => f.altitude_m ?? 0);
  }

  return fixes.map((f, i) => {
    const gpsAlt = f.altitude_m ?? 0;
    const terrainAlt = cartographics[i]!.height;
    if (!isFinite(terrainAlt)) return gpsAlt;
    return Math.max(gpsAlt, terrainAlt);
  });
}

// ────────────────────────────────────────────
// Animation marker
// ────────────────────────────────────────────

/**
 * Build a SampledPositionProperty from primary fixes and add an animated marker.
 * Configures the Cesium Clock for playback (paused by default).
 */
function setupAnimationMarker(
  viewer: Cesium.Viewer,
  primaryFixes: FixRecord[],
  heights: number[],
): Cesium.Entity {
  const startMs = primaryFixes[0]!.unix_time_ms;
  const endMs = primaryFixes[primaryFixes.length - 1]!.unix_time_ms;

  const startTime = Cesium.JulianDate.fromDate(new Date(startMs));
  const stopTime = Cesium.JulianDate.fromDate(new Date(endMs));

  // Build SampledPositionProperty from all primary fixes
  const sampledPosition = new Cesium.SampledPositionProperty();
  const times: Cesium.JulianDate[] = new Array(primaryFixes.length);
  const positions: Cesium.Cartesian3[] = new Array(primaryFixes.length);

  for (let i = 0; i < primaryFixes.length; i++) {
    const f = primaryFixes[i]!;
    times[i] = Cesium.JulianDate.fromDate(new Date(f.unix_time_ms));
    positions[i] = Cesium.Cartesian3.fromDegrees(
      f.longitude_deg,
      f.latitude_deg,
      heights[i]!,
    );
  }
  sampledPosition.addSamples(times, positions);

  // Linear interpolation (no overshoot at gaps)
  sampledPosition.setInterpolationOptions({
    interpolationDegree: 1,
    interpolationAlgorithm: Cesium.LinearApproximation,
  });

  // Configure Clock for playback
  const clock = viewer.clock;
  clock.startTime = startTime.clone();
  clock.stopTime = stopTime.clone();
  clock.currentTime = startTime.clone();
  clock.multiplier = DEFAULT_MULTIPLIER;
  clock.clockRange = Cesium.ClockRange.CLAMPED;
  clock.shouldAnimate = false; // PlaybackControls will start it

  // Add marker entity with terrain-adjusted altitude.
  // disableDepthTestDistance keeps it visible through terrain at grazing angles.
  const marker = viewer.entities.add({
    position: sampledPosition,
    point: {
      pixelSize: 14,
      color: Cesium.Color.fromCssColorString("#8ab4f8"),
      outlineColor: Cesium.Color.WHITE,
      outlineWidth: 2,
      disableDepthTestDistance: Number.POSITIVE_INFINITY,
    },
    viewFrom: new Cesium.Cartesian3(0, -200, 300),
  });

  return marker;
}

// ────────────────────────────────────────────
// Static track polyline
// ────────────────────────────────────────────

/**
 * Add track as colored polyline segments (Primary fixes only).
 * Groups consecutive fixes with similar accuracy into segments.
 * Breaks polyline at time gaps > POLYLINE_GAP_MS.
 */
function addColoredTrack(
  viewer: Cesium.Viewer,
  fixes: FixRecord[],
  heights: number[],
) {
  if (fixes.length < 2) return;

  const BUCKET_THRESHOLDS = [5, 10, 20, 50, 100];

  function bucket(acc: number | null): number {
    if (acc == null) return BUCKET_THRESHOLDS.length;
    for (let i = 0; i < BUCKET_THRESHOLDS.length; i++) {
      if (acc < BUCKET_THRESHOLDS[i]!) return i;
    }
    return BUCKET_THRESHOLDS.length;
  }

  let segStart = 0;
  let currentBucket = bucket(fixes[0]!.accuracy_m);

  for (let i = 1; i <= fixes.length; i++) {
    // Detect time gap or accuracy bucket change or end of array
    const isEnd = i === fixes.length;
    const isGap =
      !isEnd &&
      fixes[i]!.unix_time_ms - fixes[i - 1]!.unix_time_ms > POLYLINE_GAP_MS;
    const b = isEnd ? -1 : bucket(fixes[i]!.accuracy_m);
    const bucketChanged = b !== currentBucket;

    if (isEnd || isGap || bucketChanged) {
      // Flush segment [segStart, i] — include overlap point for continuity
      const end = Math.min(i, fixes.length - 1);

      if (end - segStart + 1 >= 2) {
        const positions: Cesium.Cartesian3[] = [];
        for (let j = segStart; j <= end; j++) {
          const f = fixes[j]!;
          positions.push(
            Cesium.Cartesian3.fromDegrees(
              f.longitude_deg,
              f.latitude_deg,
              heights[j]!,
            ),
          );
        }

        const midIdx = segStart + Math.floor((end - segStart) / 2);
        const color = accuracyToColor(fixes[midIdx]!.accuracy_m);

        viewer.entities.add({
          polyline: {
            positions,
            width: 3,
            material: new Cesium.ColorMaterialProperty(color),
          },
        });
      }

      segStart = i;
      currentBucket = b;
    }
  }
}

// ────────────────────────────────────────────
// NLP points
// ────────────────────────────────────────────

/**
 * Add NLP fixes as semi-transparent points with accuracy circles.
 */
function addNlpPoints(
  viewer: Cesium.Viewer,
  fixes: { fix: FixRecord; quality: FixQuality }[],
) {
  for (const { fix: f, quality } of fixes) {
    const position = Cesium.Cartesian3.fromDegrees(
      f.longitude_deg,
      f.latitude_deg,
      f.altitude_m ?? 0,
    );

    const isGapFallback = quality === "GapFallback";
    const pointColor = isGapFallback
      ? Cesium.Color.ORANGE.withAlpha(0.7)
      : Cesium.Color.RED.withAlpha(0.4);
    const circleColor = isGapFallback
      ? Cesium.Color.ORANGE.withAlpha(0.15)
      : Cesium.Color.RED.withAlpha(0.08);

    // Point marker
    viewer.entities.add({
      position,
      point: {
        pixelSize: isGapFallback ? 6 : 4,
        color: pointColor,
        outlineWidth: 0,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
      },
    });

    // Accuracy circle
    const accuracy = f.accuracy_m ?? 100;
    if (accuracy > 0) {
      viewer.entities.add({
        position: Cesium.Cartesian3.fromDegrees(
          f.longitude_deg,
          f.latitude_deg,
        ),
        ellipse: {
          semiMajorAxis: accuracy,
          semiMinorAxis: accuracy,
          material: new Cesium.ColorMaterialProperty(circleColor),
          outline: true,
          outlineColor: pointColor,
          outlineWidth: 1,
        },
      });
    }
  }
}
