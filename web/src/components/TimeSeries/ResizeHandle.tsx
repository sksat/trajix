import { useCallback, useRef } from "react";

interface ResizeHandleProps {
  onResize: (delta: number) => void;
}

/**
 * Drag handle for resizing the chart panel.
 * Uses PointerEvent + setPointerCapture for reliable cross-browser drag.
 */
export function ResizeHandle({ onResize }: ResizeHandleProps) {
  const startY = useRef(0);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      startY.current = e.clientY;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    },
    [],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!(e.target as HTMLElement).hasPointerCapture(e.pointerId)) return;
      const dy = startY.current - e.clientY; // drag up = grow
      startY.current = e.clientY;
      onResize(dy);
    },
    [onResize],
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      (e.target as HTMLElement).releasePointerCapture(e.pointerId);
    },
    [],
  );

  return (
    <div
      className="resize-handle"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      <div className="resize-handle-bar" />
    </div>
  );
}
