import { useEffect, useRef } from "react";
import * as Cesium from "cesium";
import "cesium/Build/Cesium/Widgets/widgets.css";
import type { ProcessingResult } from "../../types/gnss";
import type { FixRecord } from "../../types/gnss";
import { accuracyToColor } from "../../utils/color";
import { createGsiTerrainProvider } from "../../utils/gsiTerrain";
import { filterAltitudeSpikes, smoothAltitudes } from "../../utils/altitude";
import {
  findActiveNlpFixes,
  nlpStyle,
  nlpRenderHeight,
  type NlpFixEntry,
} from "../../utils/nlpFilter";
import "./CesiumMap.css";

// Cesium ion token from env (optional — for PLATEAU 3D buildings)
const ION_TOKEN = import.meta.env.VITE_CESIUM_ION_TOKEN as string | undefined;

/// Gap threshold for breaking polylines (ms).
/// If consecutive Primary fixes are more than this apart, break the polyline.
const POLYLINE_GAP_MS = 3000;

/// Default playback speed multiplier (50x = ~8 min for 6.5h recording).
const DEFAULT_MULTIPLIER = 50;

/// Number of reusable NLP entity slots (point + ellipse combined per entity).
const NLP_POOL_SIZE = 3;

// Pre-allocated Cesium colors for NLP rendering (avoid per-frame allocation)
const NLP_COLORS = {
  gapPoint: Cesium.Color.ORANGE.withAlpha(0.9),
  gapCircle: Cesium.Color.ORANGE.withAlpha(0.35),
  gapLine: Cesium.Color.ORANGE.withAlpha(0.6),
  rejPoint: Cesium.Color.RED.withAlpha(0.7),
  rejCircle: Cesium.Color.RED.withAlpha(0.25),
  rejLine: Cesium.Color.RED.withAlpha(0.5),
} as const;

const GSI_CREDIT = new Cesium.Credit(
  '<a href="https://maps.gsi.go.jp/development/ichiran.html">国土地理院</a>',
);

// GSI imagery tile definitions
export const GSI_IMAGERY = {
  seamlessphoto: { url: "https://cyberjapandata.gsi.go.jp/xyz/seamlessphoto/{z}/{x}/{y}.jpg", maxZoom: 18, label: "航空写真" },
  pale:          { url: "https://cyberjapandata.gsi.go.jp/xyz/pale/{z}/{x}/{y}.png",          maxZoom: 18, label: "淡色地図" },
  std:           { url: "https://cyberjapandata.gsi.go.jp/xyz/std/{z}/{x}/{y}.png",           maxZoom: 18, label: "標準地図" },
  relief:        { url: "https://cyberjapandata.gsi.go.jp/xyz/relief/{z}/{x}/{y}.png",        maxZoom: 15, label: "色別標高図" },
} as const;

export type GsiImageryKey = keyof typeof GSI_IMAGERY;

interface CesiumMapProps {
  result: ProcessingResult;
  showNlp?: boolean;
  imagery?: GsiImageryKey;
  onViewerReady?: (viewer: Cesium.Viewer) => void;
}

export function CesiumMap({
  result,
  showNlp = false,
  imagery = "seamlessphoto",
  onViewerReady,
}: CesiumMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<Cesium.Viewer | null>(null);

  // NLP time-based rendering state
  const nlpFixesRef = useRef<NlpFixEntry[]>([]);
  const nlpPoolRef = useRef<Cesium.Entity[]>([]);
  const nlpLinePoolRef = useRef<Cesium.Entity[]>([]);
  const nlpLinePositionsRef = useRef<Cesium.Cartesian3[][]>([]);
  const nlpLastRenderedRef = useRef<number[]>([]);
  /** Cached start time (unix ms) for efficient JulianDate→ms conversion. */
  const nlpStartMsRef = useRef<number>(0);
  /** Animation marker entity for connecting NLP dashed lines. */
  const markerRef = useRef<Cesium.Entity | null>(null);

  // Initialize viewer once
  useEffect(() => {
    if (!containerRef.current) return;

    if (ION_TOKEN) {
      Cesium.Ion.defaultAccessToken = ION_TOKEN;
    }

    const defaultImagery = GSI_IMAGERY.seamlessphoto;
    const gsiProvider = new Cesium.UrlTemplateImageryProvider({
      url: defaultImagery.url,
      credit: GSI_CREDIT,
      minimumLevel: 2,
      maximumLevel: defaultImagery.maxZoom,
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

    // Ensure logarithmic depth buffer is enabled for close-up terrain viewing.
    // This prevents near-plane clipping when the camera is close to terrain.
    viewer.scene.logarithmicDepthBuffer = true;

    // Depth-test entities (polylines, points, etc.) against terrain so they
    // are hidden when behind mountains instead of rendering through them.
    viewer.scene.globe.depthTestAgainstTerrain = true;

    viewerRef.current = viewer;
    // Expose viewer globally for debugging/demo recording
    (window as unknown as Record<string, unknown>).__cesiumViewer = viewer;
    onViewerReady?.(viewer);

    return () => {
      viewer.destroy();
      viewerRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Switch imagery layer when selection changes
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer) return;

    const config = GSI_IMAGERY[imagery];
    const layers = viewer.imageryLayers;
    layers.removeAll();
    layers.addImageryProvider(
      new Cesium.UrlTemplateImageryProvider({
        url: config.url,
        credit: GSI_CREDIT,
        minimumLevel: 2,
        maximumLevel: config.maxZoom,
      }),
    );
  }, [imagery]);

  // Render track + animation marker when result changes.
  // Terrain sampling is async, so we use a cancellation flag.
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer || result.fixes.length === 0) return;

    let cancelled = false;

    // Prepare sorted NLP fixes for time-based rendering
    const nlpEntries: NlpFixEntry[] = [];
    for (let i = 0; i < result.fixes.length; i++) {
      const q = result.fix_qualities[i]!;
      if (q === "GapFallback" || q === "Rejected") {
        nlpEntries.push({
          fix: result.fixes[i]!,
          quality: q as "GapFallback" | "Rejected",
        });
      }
    }
    nlpEntries.sort((a, b) => a.fix.unix_time_ms - b.fix.unix_time_ms);
    nlpFixesRef.current = nlpEntries;

    // Clear NLP pool (removeAll below destroys those entities)
    nlpPoolRef.current = [];
    nlpLinePoolRef.current = [];
    nlpLinePositionsRef.current = [];
    nlpLastRenderedRef.current = [];
    markerRef.current = null;

    const primaryFixes = result.fixes.filter(
      (_, i) => result.fix_qualities[i] === "Primary",
    );

    // Cache start time for efficient JulianDate→ms conversion
    if (primaryFixes.length > 0) {
      nlpStartMsRef.current = primaryFixes[0]!.unix_time_ms;
    }

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

      // Smooth altitude jitter (GPS vertical noise ~2-10m)
      const smoothedHeights = smoothAltitudes(primaryFixes, filteredHeights);

      viewer.entities.removeAll();

      addColoredTrack(viewer, primaryFixes, smoothedHeights);

      if (primaryFixes.length >= 2) {
        markerRef.current = setupAnimationMarker(viewer, primaryFixes, smoothedHeights);
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
  }, [result]);

  // ── Time-based NLP rendering via preRender ──
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer) return;

    if (!showNlp) {
      // Hide all pool entities when NLP layer is off
      for (const entity of nlpPoolRef.current) entity.show = false;
      for (const line of nlpLinePoolRef.current) line.show = false;
      return;
    }

    // Create pool if it doesn't exist (first toggle or after result change)
    if (nlpPoolRef.current.length === 0) {
      const pool: Cesium.Entity[] = [];
      const linePool: Cesium.Entity[] = [];
      const linePositions: Cesium.Cartesian3[][] = [];
      for (let i = 0; i < NLP_POOL_SIZE; i++) {
        // NLP station entity: point + label + accuracy ellipse
        const entity = viewer.entities.add({
          show: false,
          position: Cesium.Cartesian3.fromDegrees(0, 0, 0),
          point: {
            pixelSize: 8,
            color: NLP_COLORS.gapPoint,
            outlineColor: Cesium.Color.WHITE,
            outlineWidth: 1,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            disableDepthTestDistance: Number.POSITIVE_INFINITY,
          },
          label: {
            text: "\u{1F4E1}",
            font: "16px sans-serif",
            verticalOrigin: Cesium.VerticalOrigin.CENTER,
            showBackground: false,
            disableDepthTestDistance: Number.POSITIVE_INFINITY,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
          },
          ellipse: {
            semiMajorAxis: 100,
            semiMinorAxis: 100,
            material: new Cesium.ColorMaterialProperty(NLP_COLORS.gapCircle),
            outline: false,
            heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
          },
        });
        pool.push(entity);

        // Dashed line: use CallbackProperty to avoid per-frame property allocation
        const posArray = [
          new Cesium.Cartesian3(),
          new Cesium.Cartesian3(),
        ];
        linePositions.push(posArray);
        const lineEntity = viewer.entities.add({
          show: false,
          polyline: {
            positions: new Cesium.CallbackProperty(() => posArray, false),
            width: 2,
            material: new Cesium.PolylineDashMaterialProperty({
              color: NLP_COLORS.gapLine,
              dashLength: 16,
            }),
          },
        });
        linePool.push(lineEntity);
      }
      nlpPoolRef.current = pool;
      nlpLinePoolRef.current = linePool;
      nlpLinePositionsRef.current = linePositions;
      nlpLastRenderedRef.current = new Array(NLP_POOL_SIZE).fill(-1);
    }

    const pool = nlpPoolRef.current;
    const linePool = nlpLinePoolRef.current;
    const startMs = nlpStartMsRef.current;
    let lastTimeMs = -1;
    // Pre-allocated scratch object for NLP line endpoint conversion
    const scratchNlpCarto = new Cesium.Cartographic();

    const onPreRender = () => {
      const nlpFixes = nlpFixesRef.current;
      if (nlpFixes.length === 0) {
        for (const entity of pool) entity.show = false;
        for (const line of linePool) line.show = false;
        return;
      }

      // Convert JulianDate -> unix ms without Date allocation
      const elapsedSec = Cesium.JulianDate.secondsDifference(
        viewer.clock.currentTime,
        viewer.clock.startTime,
      );
      const currentMs = startMs + elapsedSec * 1000;

      // Gate: skip if time hasn't changed (pause optimization)
      const roundedMs = Math.round(currentMs);
      if (roundedMs === lastTimeMs) return;
      lastTimeMs = roundedMs;

      // When paused, show the most recent NLP fix regardless of linger time
      // (otherwise NLP disappears at end-of-timeline or after seeking)
      const lingerMs = viewer.clock.shouldAnimate ? undefined : Infinity;
      const active = findActiveNlpFixes(nlpFixes, currentMs, lingerMs);
      const matchCount = Math.min(active.length, pool.length);

      // Update NLP entity slots
      for (let i = 0; i < matchCount; i++) {
        const entry = active[i]!;
        const entryMs = entry.fix.unix_time_ms;

        // Skip if this slot already shows the same fix
        if (nlpLastRenderedRef.current[i] === entryMs) {
          pool[i]!.show = true;
          continue;
        }
        nlpLastRenderedRef.current[i] = entryMs;

        const f = entry.fix as FixRecord;
        const style = nlpStyle(entry.quality);
        const entity = pool[i]!;

        // Update position (always ground-clamped, never at GPS/marker altitude)
        const pos = Cesium.Cartesian3.fromDegrees(
          f.longitude_deg,
          f.latitude_deg,
          nlpRenderHeight(f.altitude_m),
        );
        (entity.position as Cesium.ConstantPositionProperty).setValue(pos);

        // Update point style
        const pointColor =
          style.hue === "orange" ? NLP_COLORS.gapPoint : NLP_COLORS.rejPoint;
        entity.point!.pixelSize =
          new Cesium.ConstantProperty(style.pointSize) as unknown as
            Cesium.Property;
        entity.point!.color =
          new Cesium.ConstantProperty(pointColor) as unknown as
            Cesium.Property;

        // Update ellipse style
        const accuracy = f.accuracy_m ?? 100;
        const circleColor =
          style.hue === "orange" ? NLP_COLORS.gapCircle : NLP_COLORS.rejCircle;
        entity.ellipse!.semiMajorAxis =
          new Cesium.ConstantProperty(accuracy) as unknown as Cesium.Property;
        entity.ellipse!.semiMinorAxis =
          new Cesium.ConstantProperty(accuracy) as unknown as Cesium.Property;
        entity.ellipse!.material = new Cesium.ColorMaterialProperty(
          circleColor,
        );

        // Update line material (only when NLP fix changes)
        if (linePool[i]) {
          const lineColor =
            style.hue === "orange" ? NLP_COLORS.gapLine : NLP_COLORS.rejLine;
          linePool[i]!.polyline!.material =
            new Cesium.PolylineDashMaterialProperty({
              color: lineColor,
              dashLength: 16,
            });
        }

        entity.show = accuracy > 0;
      }

      // Hide unused NLP entity slots
      for (let i = matchCount; i < pool.length; i++) {
        pool[i]!.show = false;
        nlpLastRenderedRef.current[i] = -1;
      }

      // Update dashed lines from NLP station to animation marker (in-place)
      const markerEntity = markerRef.current;
      const markerPos = markerEntity?.position?.getValue(
        viewer.clock.currentTime,
      );
      const linePositions = nlpLinePositionsRef.current;
      for (let i = 0; i < matchCount; i++) {
        if (!pool[i]!.show || !markerPos || !linePositions[i]) {
          linePool[i]!.show = false;
          continue;
        }
        const nlpPos = (
          pool[i]!.position as Cesium.ConstantPositionProperty
        ).getValue(viewer.clock.currentTime);
        if (nlpPos) {
          // NLP endpoint stays at ground level (height=0); line slopes up to marker
          Cesium.Cartographic.fromCartesian(
            nlpPos, undefined, scratchNlpCarto,
          );
          scratchNlpCarto.height = nlpRenderHeight(null);
          viewer.scene.globe.ellipsoid.cartographicToCartesian(
            scratchNlpCarto, linePositions[i]![0]!,
          );
          Cesium.Cartesian3.clone(markerPos, linePositions[i]![1]!);
          linePool[i]!.show = true;
        } else {
          linePool[i]!.show = false;
        }
      }
      // Hide unused line slots
      for (let i = matchCount; i < linePool.length; i++) {
        linePool[i]!.show = false;
      }
    };

    viewer.scene.preRender.addEventListener(onPreRender);

    return () => {
      if (viewer.isDestroyed()) return;
      viewer.scene.preRender.removeEventListener(onPreRender);
      for (const entity of pool) entity.show = false;
      for (const line of linePool) line.show = false;
    };
  }, [showNlp, result]);

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
  const marker = viewer.entities.add({
    position: sampledPosition,
    point: {
      pixelSize: 14,
      color: Cesium.Color.fromCssColorString("#8ab4f8"),
      outlineColor: Cesium.Color.WHITE,
      outlineWidth: 2,
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

