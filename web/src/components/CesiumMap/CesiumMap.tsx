import { useEffect, useRef } from "react";
import * as Cesium from "cesium";
import "cesium/Build/Cesium/Widgets/widgets.css";
import type { ProcessingResult } from "../../types/gnss";
import { accuracyToColor } from "../../utils/color";
import { createGsiTerrainProvider } from "../../utils/gsiTerrain";
import "./CesiumMap.css";

// Cesium ion token from env (optional — for PLATEAU 3D buildings)
const ION_TOKEN = import.meta.env.VITE_CESIUM_ION_TOKEN as string | undefined;

interface CesiumMapProps {
  result: ProcessingResult;
}

export function CesiumMap({ result }: CesiumMapProps) {
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

    // Amplify terrain relief (GSI DEM at this scale benefits from exaggeration)
    viewer.scene.verticalExaggeration = 2.0;

    viewerRef.current = viewer;

    return () => {
      viewer.destroy();
      viewerRef.current = null;
    };
  }, []);

  // Render track when result changes
  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer || result.fixes.length === 0) return;

    viewer.entities.removeAll();
    addColoredTrack(viewer, result.fixes);

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
        viewer.scene.primitives.add(tileset);
      });
    }
  }, [result]);

  return <div ref={containerRef} className="cesium-map-container" />;
}

/**
 * Add track as colored polyline segments.
 * Groups consecutive fixes with similar accuracy into segments
 * to reduce entity count while preserving color variation.
 */
function addColoredTrack(
  viewer: Cesium.Viewer,
  fixes: ProcessingResult["fixes"],
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
    const b = i < fixes.length ? bucket(fixes[i]!.accuracy_m) : -1;

    if (b !== currentBucket || i === fixes.length) {
      // Flush segment [segStart, i] — include overlap point for continuity
      const end = Math.min(i, fixes.length - 1);
      const segFixes = fixes.slice(segStart, end + 1);

      if (segFixes.length >= 2) {
        const positions = segFixes.map((f) =>
          Cesium.Cartesian3.fromDegrees(
            f.longitude_deg,
            f.latitude_deg,
            f.altitude_m ?? 0,
          ),
        );

        const midFix = segFixes[Math.floor(segFixes.length / 2)]!;
        const color = accuracyToColor(midFix.accuracy_m);

        viewer.entities.add({
          polyline: {
            positions,
            width: 3,
            material: new Cesium.ColorMaterialProperty(color),
            clampToGround: true,
          },
        });
      }

      segStart = i;
      currentBucket = b;
    }
  }
}
