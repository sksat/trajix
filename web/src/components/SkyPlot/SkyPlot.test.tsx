// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { SkyPlot } from "./SkyPlot";
import type { SatelliteSnapshotJs } from "../../types/gnss";

function makeSatSnapshot(
  overrides?: Partial<SatelliteSnapshotJs>,
): SatelliteSnapshotJs {
  return {
    time_ms: 1000,
    constellation: 1,
    svid: 10,
    cn0_dbhz: 30,
    azimuth_deg: 45,
    elevation_deg: 60,
    used_in_fix: true,
    ...overrides,
  };
}

describe("SkyPlot component", () => {
  it("renders legend with all constellation labels", () => {
    const { container } = render(
      <SkyPlot snapshots={[]} currentTimeMs={0} />,
    );
    const legend = container.querySelector(".sky-plot-legend");
    expect(legend).not.toBeNull();
    expect(legend!.textContent).toContain("GPS");
    expect(legend!.textContent).toContain("GLO");
    expect(legend!.textContent).toContain("GAL");
    expect(legend!.textContent).toContain("BDS");
    expect(legend!.textContent).toContain("QZS");
  });

  it("renders satellite markers for given time", () => {
    const snapshots: SatelliteSnapshotJs[] = [
      makeSatSnapshot({ constellation: 1, svid: 10, time_ms: 1000 }),
      makeSatSnapshot({ constellation: 6, svid: 4, time_ms: 1000 }),
    ];
    const { container } = render(
      <SkyPlot snapshots={snapshots} currentTimeMs={1000} />,
    );
    const labels = container.querySelectorAll(".sat-label");
    const texts = Array.from(labels).map((el) => el.textContent);
    expect(texts).toContain("G10");
    expect(texts).toContain("E4");
  });

  it("does not render satellite markers outside time window", () => {
    const snapshots: SatelliteSnapshotJs[] = [
      makeSatSnapshot({ time_ms: 5000 }),
    ];
    const { container } = render(
      <SkyPlot snapshots={snapshots} currentTimeMs={1000} />,
    );
    const labels = container.querySelectorAll(".sat-label");
    expect(labels.length).toBe(0);
  });

  it("clicking does not toggle collapsed class", () => {
    const { container } = render(
      <SkyPlot snapshots={[]} currentTimeMs={0} />,
    );
    const root = container.querySelector(".sky-plot")!;
    expect(root.classList.contains("collapsed")).toBe(false);
    fireEvent.click(root);
    expect(root.classList.contains("collapsed")).toBe(false);
  });

  it("legend is always in the DOM (not conditionally rendered)", () => {
    const { container } = render(
      <SkyPlot snapshots={[]} currentTimeMs={0} />,
    );
    const legendItems = container.querySelectorAll(".sky-legend-item");
    expect(legendItems.length).toBe(5);
  });
});
