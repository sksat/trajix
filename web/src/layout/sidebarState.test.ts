import { describe, it, expect } from "vitest";
import {
  type SidebarState,
  MOBILE_BREAKPOINT,
  MOBILE_PEEK_HEIGHT,
  isMobile,
  initialSidebarState,
  desktopToggle,
  mobileToggle,
  shouldRenderContent,
  chartsInSidebar,
  sidebarWrapperClass,
  sidebarClass,
} from "./sidebarState";

describe("sidebar responsive state", () => {
  // ── Breakpoint detection ──

  describe("isMobile", () => {
    it("returns true below breakpoint", () => {
      expect(isMobile(390)).toBe(true);
      expect(isMobile(767)).toBe(true);
    });

    it("returns false at breakpoint", () => {
      expect(isMobile(768)).toBe(false);
    });

    it("returns false above breakpoint", () => {
      expect(isMobile(1024)).toBe(false);
      expect(isMobile(1920)).toBe(false);
    });
  });

  // ── Initial state ──

  describe("initialSidebarState", () => {
    it("desktop: sidebar visible (not collapsed)", () => {
      const s = initialSidebarState(1024);
      expect(s.collapsed).toBe(false);
      expect(s.open).toBe(false);
    });

    it("mobile: sidebar collapsed by default", () => {
      const s = initialSidebarState(390);
      expect(s.collapsed).toBe(true);
      expect(s.open).toBe(false);
    });

    it("boundary: 768px is desktop", () => {
      expect(initialSidebarState(768).collapsed).toBe(false);
    });

    it("boundary: 767px is mobile", () => {
      expect(initialSidebarState(767).collapsed).toBe(true);
    });
  });

  // ── Desktop collapse toggle ──

  describe("desktopToggle", () => {
    it("collapse: visible → collapsed", () => {
      const s = desktopToggle({ open: false, collapsed: false });
      expect(s.collapsed).toBe(true);
      expect(s.open).toBe(false);
    });

    it("expand: collapsed → visible", () => {
      const s = desktopToggle({ open: false, collapsed: true });
      expect(s.collapsed).toBe(false);
      expect(s.open).toBe(true);
    });

    it("round-trip: toggle twice returns to original collapsed value", () => {
      const original: SidebarState = { open: false, collapsed: false };
      const result = desktopToggle(desktopToggle(original));
      expect(result.collapsed).toBe(original.collapsed);
    });
  });

  // ── Mobile drawer toggle ──

  describe("mobileToggle", () => {
    it("open drawer: collapsed → expanded", () => {
      const s = mobileToggle({ open: false, collapsed: true });
      expect(s.open).toBe(true);
      expect(s.collapsed).toBe(false);
    });

    it("close drawer: expanded → collapsed", () => {
      const s = mobileToggle({ open: true, collapsed: false });
      expect(s.open).toBe(false);
      expect(s.collapsed).toBe(true);
    });

    it("round-trip: toggle twice returns to original", () => {
      const original: SidebarState = { open: false, collapsed: true };
      const result = mobileToggle(mobileToggle(original));
      expect(result.open).toBe(original.open);
      expect(result.collapsed).toBe(original.collapsed);
    });
  });

  // ── State invariants ──

  describe("state invariants", () => {
    it("open and collapsed are always inverse after desktop toggle", () => {
      let s: SidebarState = { open: false, collapsed: false };
      for (let i = 0; i < 10; i++) {
        s = desktopToggle(s);
        expect(s.open).toBe(!s.collapsed);
      }
    });

    it("open and collapsed are always inverse after mobile toggle", () => {
      let s: SidebarState = { open: false, collapsed: true };
      for (let i = 0; i < 10; i++) {
        s = mobileToggle(s);
        expect(s.open).toBe(!s.collapsed);
      }
    });

    it("desktop and mobile toggles produce same result from same state", () => {
      const fromCollapsed: SidebarState = { open: false, collapsed: true };
      expect(desktopToggle(fromCollapsed)).toEqual(mobileToggle(fromCollapsed));

      const fromExpanded: SidebarState = { open: true, collapsed: false };
      expect(desktopToggle(fromExpanded)).toEqual(mobileToggle(fromExpanded));
    });
  });

  // ── Content rendering ──

  describe("shouldRenderContent", () => {
    it("renders when not collapsed", () => {
      expect(shouldRenderContent({ open: true, collapsed: false })).toBe(true);
    });

    it("hidden when collapsed", () => {
      expect(shouldRenderContent({ open: false, collapsed: true })).toBe(false);
    });

    it("desktop initial: content visible", () => {
      expect(shouldRenderContent(initialSidebarState(1024))).toBe(true);
    });

    it("mobile initial: content hidden", () => {
      expect(shouldRenderContent(initialSidebarState(390))).toBe(false);
    });
  });

  // ── Chart placement ──

  describe("chartsInSidebar", () => {
    it("mobile: charts in sidebar drawer", () => {
      expect(chartsInSidebar(390)).toBe(true);
    });

    it("desktop: charts in standalone panel", () => {
      expect(chartsInSidebar(1024)).toBe(false);
    });

    it("boundary: 768px → standalone", () => {
      expect(chartsInSidebar(768)).toBe(false);
    });

    it("boundary: 767px → in sidebar", () => {
      expect(chartsInSidebar(767)).toBe(true);
    });
  });

  // ── CSS class derivation ──

  describe("sidebarWrapperClass", () => {
    it("adds .collapsed when sidebar is collapsed", () => {
      expect(sidebarWrapperClass({ open: false, collapsed: true })).toBe(
        "sidebar-wrapper collapsed",
      );
    });

    it("no .collapsed when sidebar is visible", () => {
      expect(sidebarWrapperClass({ open: true, collapsed: false })).toBe(
        "sidebar-wrapper",
      );
    });
  });

  describe("sidebarClass", () => {
    it("adds .expanded when drawer is open", () => {
      expect(sidebarClass({ open: true, collapsed: false })).toBe(
        "sidebar expanded",
      );
    });

    it("no .expanded when drawer is closed", () => {
      expect(sidebarClass({ open: false, collapsed: true })).toBe("sidebar");
    });
  });

  // ── Full lifecycle scenarios ──

  describe("mobile lifecycle", () => {
    it("open → close cycle", () => {
      // Start: mobile default
      let s = initialSidebarState(390);
      expect(s).toEqual({ open: false, collapsed: true });
      expect(shouldRenderContent(s)).toBe(false);
      expect(sidebarClass(s)).toBe("sidebar");

      // Tap handle → drawer opens
      s = mobileToggle(s);
      expect(s).toEqual({ open: true, collapsed: false });
      expect(shouldRenderContent(s)).toBe(true);
      expect(sidebarClass(s)).toBe("sidebar expanded");

      // Tap handle → drawer closes
      s = mobileToggle(s);
      expect(s).toEqual({ open: false, collapsed: true });
      expect(shouldRenderContent(s)).toBe(false);
      expect(sidebarClass(s)).toBe("sidebar");
    });
  });

  describe("desktop lifecycle", () => {
    it("collapse → expand cycle", () => {
      // Start: desktop default
      let s = initialSidebarState(1024);
      expect(s).toEqual({ open: false, collapsed: false });
      expect(shouldRenderContent(s)).toBe(true);
      expect(sidebarWrapperClass(s)).toBe("sidebar-wrapper");

      // Click collapse tab
      s = desktopToggle(s);
      expect(s).toEqual({ open: false, collapsed: true });
      expect(shouldRenderContent(s)).toBe(false);
      expect(sidebarWrapperClass(s)).toBe("sidebar-wrapper collapsed");

      // Click expand tab
      s = desktopToggle(s);
      expect(s).toEqual({ open: true, collapsed: false });
      expect(shouldRenderContent(s)).toBe(true);
      expect(sidebarWrapperClass(s)).toBe("sidebar-wrapper");
    });
  });

  // ── Constants ──

  describe("constants", () => {
    it("mobile breakpoint is 768px", () => {
      expect(MOBILE_BREAKPOINT).toBe(768);
    });

    it("mobile peek height is 36px", () => {
      expect(MOBILE_PEEK_HEIGHT).toBe(36);
    });
  });
});
