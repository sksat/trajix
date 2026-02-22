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

describe("SkyPlot constellation filter", () => {
  const mixedSnapshots: SatelliteSnapshotJs[] = [
    makeSatSnapshot({ constellation: 1, svid: 10, time_ms: 1000 }), // GPS
    makeSatSnapshot({ constellation: 1, svid: 12, time_ms: 1000 }), // GPS
    makeSatSnapshot({ constellation: 3, svid: 8, time_ms: 1000 }),  // GLONASS
    makeSatSnapshot({ constellation: 6, svid: 4, time_ms: 1000 }),  // Galileo
    makeSatSnapshot({ constellation: 5, svid: 20, time_ms: 1000 }), // BeiDou
  ];

  function getSatLabels(container: HTMLElement): string[] {
    return Array.from(container.querySelectorAll(".sat-label")).map(
      (el) => el.textContent!,
    );
  }

  function clickLegend(container: HTMLElement, label: string): void {
    const items = container.querySelectorAll(".sky-legend-item");
    const item = Array.from(items).find((el) => el.textContent === label);
    expect(item).not.toBeUndefined();
    fireEvent.click(item!);
  }

  it("clicking GPS legend shows only GPS satellites", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    // Initially all 5 are shown
    expect(getSatLabels(container).length).toBe(5);

    clickLegend(container, "GPS");

    const labels = getSatLabels(container);
    expect(labels).toContain("G10");
    expect(labels).toContain("G12");
    expect(labels.length).toBe(2);
  });

  it("clicking same legend item again shows all satellites", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    expect(getSatLabels(container).length).toBe(2);

    clickLegend(container, "GPS");
    expect(getSatLabels(container).length).toBe(5);
  });

  it("clicking second legend adds to filter (multi-select)", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    expect(getSatLabels(container).length).toBe(2);

    clickLegend(container, "GLO");
    const labels = getSatLabels(container);
    expect(labels).toContain("G10");
    expect(labels).toContain("G12");
    expect(labels).toContain("R8");
    expect(labels.length).toBe(3);
  });

  it("toggling off one keeps the other active", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    clickLegend(container, "GLO");
    expect(getSatLabels(container).length).toBe(3);

    clickLegend(container, "GPS"); // toggle GPS off
    const labels = getSatLabels(container);
    expect(labels).toContain("R8");
    expect(labels.length).toBe(1);
  });

  it("toggling off last active filter shows all satellites", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    clickLegend(container, "GPS"); // toggle off → back to all
    expect(getSatLabels(container).length).toBe(5);
  });

  it("multiple legend items get 'active' class", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    clickLegend(container, "GAL");

    const items = container.querySelectorAll(".sky-legend-item");
    const activeLabels = Array.from(items)
      .filter((el) => el.classList.contains("active"))
      .map((el) => el.textContent);
    expect(activeLabels).toContain("GPS");
    expect(activeLabels).toContain("GAL");
    expect(activeLabels.length).toBe(2);
  });

  it("inactive legend items get 'inactive' class when filter is active", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");

    const items = container.querySelectorAll(".sky-legend-item");
    const inactiveItems = Array.from(items).filter((el) =>
      el.classList.contains("inactive"),
    );
    // 4 out of 5 should be inactive (GLO, GAL, BDS, QZS)
    expect(inactiveItems.length).toBe(4);
  });

  it("clearing all filters removes active/inactive classes", () => {
    const { container } = render(
      <SkyPlot snapshots={mixedSnapshots} currentTimeMs={1000} />,
    );
    clickLegend(container, "GPS");
    clickLegend(container, "GPS"); // toggle off

    const items = container.querySelectorAll(".sky-legend-item");
    const anyActive = Array.from(items).some((el) =>
      el.classList.contains("active") || el.classList.contains("inactive"),
    );
    expect(anyActive).toBe(false);
  });
});
