import { describe, it, expect } from "vitest";
import {
  type PlaybackElement,
  isCompactLayout,
  elementRow,
  rowElements,
  playbackContainerClass,
} from "./playbackLayout";

describe("playback layout", () => {
  // ── Compact layout detection ──

  describe("isCompactLayout", () => {
    it("returns true below 768px", () => {
      expect(isCompactLayout(390)).toBe(true);
      expect(isCompactLayout(767)).toBe(true);
    });

    it("returns false at 768px", () => {
      expect(isCompactLayout(768)).toBe(false);
    });

    it("returns false above 768px", () => {
      expect(isCompactLayout(1024)).toBe(false);
      expect(isCompactLayout(1920)).toBe(false);
    });
  });

  // ── Element row assignment ──

  describe("elementRow", () => {
    describe("desktop (single row)", () => {
      it("all elements in row 1", () => {
        const elements: PlaybackElement[] = [
          "play",
          "speed",
          "seek",
          "time",
          "follow",
        ];
        for (const el of elements) {
          expect(elementRow(el, 1024)).toBe(1);
        }
      });
    });

    describe("mobile (two rows)", () => {
      it("play, speed, time, follow in row 1 (top)", () => {
        expect(elementRow("play", 390)).toBe(1);
        expect(elementRow("speed", 390)).toBe(1);
        expect(elementRow("time", 390)).toBe(1);
        expect(elementRow("follow", 390)).toBe(1);
      });

      it("seek alone in row 2 (bottom)", () => {
        expect(elementRow("seek", 390)).toBe(2);
      });
    });

    describe("boundary", () => {
      it("767px is compact (two rows)", () => {
        expect(elementRow("play", 767)).toBe(1);
        expect(elementRow("seek", 767)).toBe(2);
      });

      it("768px is desktop (single row)", () => {
        expect(elementRow("seek", 768)).toBe(1);
        expect(elementRow("play", 768)).toBe(1);
      });
    });
  });

  // ── Row element lists ──

  describe("rowElements", () => {
    describe("desktop", () => {
      it("row 1 contains all elements in order", () => {
        expect(rowElements(1, 1024)).toEqual([
          "play",
          "speed",
          "seek",
          "time",
          "follow",
        ]);
      });

      it("row 2 is empty", () => {
        expect(rowElements(2, 1024)).toEqual([]);
      });
    });

    describe("mobile", () => {
      it("row 1 has play, speed, time, follow", () => {
        expect(rowElements(1, 390)).toEqual(["play", "speed", "time", "follow"]);
      });

      it("row 2 has seek only (full width)", () => {
        expect(rowElements(2, 390)).toEqual(["seek"]);
      });
    });

    describe("invariants", () => {
      it("all 5 elements present across both rows (mobile)", () => {
        const all = [...rowElements(1, 390), ...rowElements(2, 390)].sort();
        expect(all).toEqual(["follow", "play", "seek", "speed", "time"]);
      });

      it("all 5 elements present across both rows (desktop)", () => {
        const all = [...rowElements(1, 1024), ...rowElements(2, 1024)].sort();
        expect(all).toEqual(["follow", "play", "seek", "speed", "time"]);
      });

      it("no element appears in both rows", () => {
        const r1 = new Set(rowElements(1, 390));
        const r2 = new Set(rowElements(2, 390));
        for (const el of r1) {
          expect(r2.has(el)).toBe(false);
        }
      });
    });
  });

  // ── CSS class derivation ──

  describe("playbackContainerClass", () => {
    it("desktop: no compact class", () => {
      expect(playbackContainerClass(1024)).toBe("playback-controls");
    });

    it("mobile: adds playback-compact class", () => {
      expect(playbackContainerClass(390)).toBe(
        "playback-controls playback-compact",
      );
    });

    it("boundary: 768px is desktop", () => {
      expect(playbackContainerClass(768)).toBe("playback-controls");
    });

    it("boundary: 767px is mobile compact", () => {
      expect(playbackContainerClass(767)).toBe(
        "playback-controls playback-compact",
      );
    });
  });

  // ── Full layout scenarios ──

  describe("layout scenario: 390px phone", () => {
    it("produces correct two-row layout", () => {
      const w = 390;
      expect(isCompactLayout(w)).toBe(true);
      expect(playbackContainerClass(w)).toBe(
        "playback-controls playback-compact",
      );
      expect(rowElements(1, w)).toEqual(["play", "speed", "time", "follow"]);
      expect(rowElements(2, w)).toEqual(["seek"]);
    });
  });

  describe("layout scenario: 1920px desktop", () => {
    it("produces single-row layout", () => {
      const w = 1920;
      expect(isCompactLayout(w)).toBe(false);
      expect(playbackContainerClass(w)).toBe("playback-controls");
      expect(rowElements(1, w)).toEqual([
        "play",
        "speed",
        "seek",
        "time",
        "follow",
      ]);
      expect(rowElements(2, w)).toEqual([]);
    });
  });
});
