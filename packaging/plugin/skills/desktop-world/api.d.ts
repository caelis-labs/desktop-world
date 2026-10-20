/** Authoring reference for the JS string passed to desktop-world exec.
 * This file is documentation for editors and Agents; no TypeScript/Node runs in DTW.
 * Upstream CUA prelude: crates/cua-driver-core/src/script_tool/prelude.js at cua-driver-rs-v0.37.0 (2d2f99228d4d).
 */
type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Point = [x: number, y: number];
type Rect = [x1: number, y1: number, x2: number, y2: number];
type Delivery = { delivery_mode?: "background" | "foreground" };
type ElementSelector = {
  role?: string;
  name?: string;
  label?: string;
  nth?: number;
  element_token?: string;
  timeout_ms?: number;
};
type Target = ElementSelector | string | number | Point;
/** Arrays only. Both spellings are accepted by DTW; never supply both. */
type Modifier = "cmd" | "command" | "super" | "meta" | "win" | "shift" |
                "option" | "alt" | "ctrl" | "control";
type ModifierOptions =
  | { modifiers?: Modifier[]; modifier?: never }
  | { modifier?: Modifier[]; modifiers?: never };
type ElementClickOptions = Delivery & ModifierOptions & {
  button?: "left" | "right" | "middle";
};
/** One-use screenshot binding, valid only with a Point target. Zoom pixels
 * cannot be combined with capture_id. */
type ClickOptions = ElementClickOptions & (
  | { from_zoom?: boolean; capture_id?: never }
  | { capture_id: string; from_zoom?: false }
);
type WindowsZoomOptions = {
  /** Windows pixel targets only, after zoom(); macOS rejects this option. */
  from_zoom?: boolean;
};
/** No capture_id, button or modifiers on doubleClick. */
type DoubleClickOptions = Delivery & WindowsZoomOptions;
/** No capture_id or button. Modifiers require a Point target.
 * For a screenshot-bound right click use click(point, {button:"right", capture_id}). */
type RightClickOptions = Delivery & ModifierOptions & WindowsZoomOptions;
type WindowInfo = {
  pid: number;
  window_id: number;
  title?: string;
  app_name?: string;
  layer?: number;
  z_index?: number | null;
  is_on_screen?: boolean;
  on_current_space?: boolean | null;
  bounds?: { x: number; y: number; width: number; height: number };
};
type Element = {
  element_token?: string;
  role?: string;
  label?: string;
  value?: Json;
  enabled?: boolean;
  selected?: boolean;
  display_only?: boolean;
  /** Screen points; use screenshot_frame for a region drawn on this response's PNG. */
  frame?: { x: number; y: number; w?: number; h?: number };
  /** This response's window screenshot pixels, including any image downscaling. */
  screenshot_frame?: { x: number; y: number; w: number; h: number };
  actions?: string[];
};
type WindowState = {
  snapshot_id?: string;
  tree_markdown?: string;
  tree_diff?: string;
  elements?: Element[];
  element_count?: number;
  truncated?: boolean;
  /** macOS geometry verdict. Windows omits this flag; require capture_id,
   * positive dimensions, no screenshot_error and a screenshot_frame, then visually
   * check alignment. DPI-unaware Windows captures can have misaligned pixels. */
  screenshot_frame_valid?: boolean;
  capture_id?: string;
  screenshot_width?: number;
  screenshot_height?: number;
  screenshot_error?: string;
};
type StateOptions = {
  query?: string;
  since?: "latest" | string;
  max_elements?: number;
  max_depth?: number;
  tree_format?: "markdown" | "elements" | "both";
  include_screenshot?: boolean;
  include_accessibility_tree?: boolean;
  display_only?: boolean;
  max_image_dimension?: number;
};

interface CuaApp {
  readonly pid: number;
  /** Updated by getState(); without windowId binding, follows the app's front window. */
  windowId: number;
  title: string | null;
  readonly pinned: boolean;
  getState(options?: StateOptions): Promise<WindowState>;
  query(selector?: ElementSelector & { text?: string }): Promise<Element[]>;
  waitFor(selector: ElementSelector & { text?: string; gone?: boolean; value?: string },
          options?: { timeout_ms?: number }): Promise<Json>;
  verify(expect: Json | Json[], options?: Json): Promise<Json>;
  /** macOS modifiers require an exact window binding and foreground delivery.
   * Dispatch does not establish GUI effect; reobserve before further input. */
  click(target: Point, options?: ClickOptions): Promise<Json>;
  click(target: Target, options?: ElementClickOptions): Promise<Json>;
  doubleClick(target: Point, options?: DoubleClickOptions): Promise<Json>;
  doubleClick(target: Target, options?: Delivery): Promise<Json>;
  rightClick(target: Point, options?: RightClickOptions): Promise<Json>;
  rightClick(target: Target, options?: Delivery): Promise<Json>;
  /** Text first; for a canvas, a Point focuses and types in one native call. */
  typeText(text: string, target?: Target, options?: Delivery): Promise<Json>;
  pressKey(key: string, target?: Target, options?: Delivery): Promise<Json>;
  hotkey(keys: string[] | string, target?: Target, options?: Delivery): Promise<Json>;
  scroll(target: Target, direction?: "up" | "down" | "left" | "right",
         amount?: number, options?: Delivery): Promise<Json>;
  /** Selector first; writes an exposed AX value, which may not commit a form. */
  setValue(target: Target, value: string, options?: Delivery): Promise<Json>;
  drag(from: Point, to: Point, options?: Delivery): Promise<Json>;
  /** Requires a valid screenshot of this window; retains a cropped image. */
  zoom(rect: Rect | { x1: number; y1: number; x2: number; y2: number }): Promise<Json>;
}

interface CuaScriptAPI {
  readonly computer: { target: "mac" | "windows" | "linux" };
  /** A PID-only binding follows the app's front window, including a native picker. */
  getApp(which: string | { pid?: number; windowId?: number; window_id?: number;
                          app?: string; title?: string }): Promise<CuaApp>;
  launch(which: string | Json, options?: Json): Promise<CuaApp | Json>;
  listApps(options?: Json): Promise<Json[]>;
  listWindows(options?: { pid?: number }): Promise<WindowInfo[]>;
  sleep(ms: number): Promise<void>;
  /** Upstream run_script allowlist only. Not invoke_menu, get_desktop_state,
   * run_steps, browser_*, configuration, or another run_script. */
  call(tool: "get_app" | "find" | "wait_for" | "sleep" |
             "click" | "double_click" | "right_click" | "set_value" |
             "type_text" | "press_key" | "hotkey" | "scroll" | "drag" |
             "move_cursor" | "launch_app" | "list_apps" | "list_windows" |
             "get_window_state" | "verify_state" | "zoom", args?: Json): Promise<Json>;
}

/** Injected by upstream QuickJS and the thin adapter around each script body. */
declare const cua: CuaScriptAPI;
declare const state: Record<string, Json>;

/** The same MCP tool carries all results. `result` with part=image displays
 * a retained screenshot; part=structured pages the complete native data. */
type ExecInput =
  | { operation?: "run"; script: string; wait_ms?: number;
      include_image?: boolean }
  | { operation: "menu"; pid: number; window_id: number;
      path: string[]; wait_ms?: number }
  | { operation: "desktop"; max_image_dimension?: number;
      wait_ms?: number; include_image?: boolean }
  | { operation: "focus"; pid: number; window_id?: number;
      wait_ms?: number }
  | { operation: "cursor"; enabled?: boolean; wait_ms?: number }
  | { operation: "app"; pid: number; max_windows?: number;
      max_image_dimension?: number; wait_ms?: number; include_image?: boolean }
  | { operation: "click"; observation_id: string; image_index: number;
      x: number; y: number; wait_ms?: number }
  | { operation: "permissions" }
  | { operation: "status" | "cancel"; id: string }
  | { operation: "result"; id: string;
      part?: "summary" | "text" | "structured" | "image";
      offset?: number; image_index?: number };
