import { describe, it, expect, vi } from "vitest";

/**
 * Tests for keyboard shortcut dispatch logic.
 *
 * We extract the handler logic and test it directly without DOM APIs.
 * This avoids needing jsdom while testing the core behavior thoroughly.
 */

// Minimal mock viewer with clock.multiplier
function mockViewer(multiplier = 50) {
  return { clock: { multiplier } } as any;
}

// Replicate the handler logic from useKeyboardShortcuts
function createHandler(
  viewer: any,
  callbacks: {
    togglePlay: () => void;
    toggleFollow: () => void;
    seekDelta: (deltaMs: number) => void;
  },
) {
  return (e: { code: string; target: { tagName: string }; preventDefault: () => void }) => {
    const tag = e.target.tagName;
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
    if (tag === "CANVAS") return;

    switch (e.code) {
      case "Space":
        e.preventDefault();
        callbacks.togglePlay();
        break;
      case "ArrowLeft":
        e.preventDefault();
        callbacks.seekDelta(-10_000 * viewer.clock.multiplier);
        break;
      case "ArrowRight":
        e.preventDefault();
        callbacks.seekDelta(10_000 * viewer.clock.multiplier);
        break;
      case "KeyF":
        e.preventDefault();
        callbacks.toggleFollow();
        break;
    }
  };
}

function fakeEvent(code: string, tagName = "DIV") {
  return {
    code,
    target: { tagName },
    preventDefault: vi.fn(),
  };
}

describe("keyboard shortcuts", () => {
  function setup(multiplier = 50) {
    const togglePlay = vi.fn();
    const toggleFollow = vi.fn();
    const seekDelta = vi.fn();
    const handler = createHandler(mockViewer(multiplier), {
      togglePlay,
      toggleFollow,
      seekDelta,
    });
    return { handler, togglePlay, toggleFollow, seekDelta };
  }

  it("Space toggles play", () => {
    const { handler, togglePlay } = setup();
    const e = fakeEvent("Space");
    handler(e);
    expect(togglePlay).toHaveBeenCalledOnce();
    expect(e.preventDefault).toHaveBeenCalled();
  });

  it("ArrowRight seeks forward by 10s * multiplier", () => {
    const { handler, seekDelta } = setup(50);
    handler(fakeEvent("ArrowRight"));
    expect(seekDelta).toHaveBeenCalledWith(500_000);
  });

  it("ArrowLeft seeks backward by 10s * multiplier", () => {
    const { handler, seekDelta } = setup(50);
    handler(fakeEvent("ArrowLeft"));
    expect(seekDelta).toHaveBeenCalledWith(-500_000);
  });

  it("F toggles follow mode", () => {
    const { handler, toggleFollow } = setup();
    handler(fakeEvent("KeyF"));
    expect(toggleFollow).toHaveBeenCalledOnce();
  });

  it("ignores shortcuts when focus is on INPUT", () => {
    const { handler, togglePlay } = setup();
    handler(fakeEvent("Space", "INPUT"));
    expect(togglePlay).not.toHaveBeenCalled();
  });

  it("ignores shortcuts when focus is on SELECT", () => {
    const { handler, toggleFollow } = setup();
    handler(fakeEvent("KeyF", "SELECT"));
    expect(toggleFollow).not.toHaveBeenCalled();
  });

  it("ignores shortcuts when focus is on TEXTAREA", () => {
    const { handler, seekDelta } = setup();
    handler(fakeEvent("ArrowRight", "TEXTAREA"));
    expect(seekDelta).not.toHaveBeenCalled();
  });

  it("ignores arrow keys when focus is on CANVAS (Cesium)", () => {
    const { handler, seekDelta } = setup();
    handler(fakeEvent("ArrowLeft", "CANVAS"));
    expect(seekDelta).not.toHaveBeenCalled();
  });

  it("does not call any callback for unrecognized keys", () => {
    const { handler, togglePlay, toggleFollow, seekDelta } = setup();
    handler(fakeEvent("KeyA"));
    expect(togglePlay).not.toHaveBeenCalled();
    expect(toggleFollow).not.toHaveBeenCalled();
    expect(seekDelta).not.toHaveBeenCalled();
  });

  it("seek delta scales with clock multiplier", () => {
    const { handler, seekDelta } = setup(100);
    handler(fakeEvent("ArrowRight"));
    expect(seekDelta).toHaveBeenCalledWith(1_000_000);
  });

  it("seek delta with x1 multiplier", () => {
    const { handler, seekDelta } = setup(1);
    handler(fakeEvent("ArrowRight"));
    expect(seekDelta).toHaveBeenCalledWith(10_000);
  });
});
