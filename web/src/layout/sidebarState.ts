/**
 * Sidebar state machine for responsive layout.
 *
 * Two synced boolean states control the sidebar:
 * - `collapsed`: controls content rendering + desktop `.collapsed` class
 * - `open`: controls mobile `.expanded` class (drawer slide-up)
 *
 * Invariant after any toggle: `open === !collapsed`
 */

export interface SidebarState {
  open: boolean;
  collapsed: boolean;
}

/** Mobile breakpoint (px). Below this → bottom drawer; at/above → right sidebar. */
export const MOBILE_BREAKPOINT = 768;

/** Height (px) of the sidebar peek area on mobile (visible when collapsed). */
export const MOBILE_PEEK_HEIGHT = 36;

export function isMobile(
  viewportWidth: number,
  viewportHeight?: number,
): boolean {
  if (viewportWidth < MOBILE_BREAKPOINT) return true;
  // Portrait orientation → mobile layout regardless of absolute width
  if (viewportHeight != null && viewportHeight > viewportWidth) return true;
  return false;
}

/** Initial state: collapsed on mobile, visible on desktop. */
export function initialSidebarState(
  viewportWidth: number,
  viewportHeight?: number,
): SidebarState {
  return {
    open: false,
    collapsed: isMobile(viewportWidth, viewportHeight),
  };
}

/** Desktop collapse-tab click: toggle collapsed, sync open. */
export function desktopToggle(state: SidebarState): SidebarState {
  const newCollapsed = !state.collapsed;
  return { collapsed: newCollapsed, open: !newCollapsed };
}

/** Mobile drag-handle click: toggle open, sync collapsed. */
export function mobileToggle(state: SidebarState): SidebarState {
  const newOpen = !state.open;
  return { open: newOpen, collapsed: !newOpen };
}

/** Whether sidebar content should be rendered (hidden when collapsed). */
export function shouldRenderContent(state: SidebarState): boolean {
  return !state.collapsed;
}

/** Charts render inside sidebar on mobile, standalone panel on desktop. */
export function chartsInSidebar(
  viewportWidth: number,
  viewportHeight?: number,
): boolean {
  return isMobile(viewportWidth, viewportHeight);
}

/** CSS class for sidebar wrapper element. */
export function sidebarWrapperClass(state: SidebarState): string {
  return `sidebar-wrapper${state.collapsed ? " collapsed" : ""}`;
}

/** CSS class for sidebar element. */
export function sidebarClass(state: SidebarState): string {
  return `sidebar${state.open ? " expanded" : ""}`;
}
