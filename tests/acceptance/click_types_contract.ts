/// <reference path="../../packaging/plugin/skills/desktop-world/api.d.ts" />
declare const app: CuaApp;
app.click([1, 2], {modifiers: ["ctrl"], delivery_mode: "foreground"});
app.click([1, 2], {modifier: ["shift"], capture_id: "fresh", button: "right"});
app.click({name: "fixture"}, {button: "right"});
app.click([1, 2], {from_zoom: true});
app.rightClick([1, 2], {modifier: ["ctrl"]});
app.rightClick({name: "fixture"});
app.doubleClick([1, 2], {delivery_mode: "foreground"});
app.typeText("fixture", [1, 2], {delivery_mode: "foreground"});
app.click([1, 2], {capture_id: "fresh", count: 2});
app.typeText("fixture", {name: "field"}, {delay_ms: 10});
app.scroll(undefined, "down", 1, {by: "page"});
app.drag([1, 2], [3, 4], {duration_ms: 500, steps: 20, button: "left", from_zoom: true});
app.getState({verbose: true, full_output: true, timeout_ms: 1000, max_dimension: 800});
const binding: {pid: number; window_id?: number} = app.where();
const snapshot: string | null = app.snapshotId;
const appName: string | null = app.name;
app.window();
// @ts-expect-error count applies only to pixel targets
app.click({name: "fixture"}, {count: 2});
// @ts-expect-error count is bounded to the portable native contract
app.click([1, 2], {count: 4});
// @ts-expect-error scroll granularity is line or page
app.scroll([1, 2], "down", 1, {by: "pixel"});
// @ts-expect-error drag has no screenshot capture binding
app.drag([1, 2], [3, 4], {capture_id: "fresh"});
// @ts-expect-error modifier arrays cannot silently become plain clicks
app.click([1, 2], {modifier: "ctrl"});
// @ts-expect-error aliases are mutually exclusive
app.click([1, 2], {modifier: ["ctrl"], modifiers: ["shift"]});
// @ts-expect-error unknown modifier names are rejected
app.click([1, 2], {modifiers: ["typo"]});
// @ts-expect-error captures bind pixel points only
app.click({name: "fixture"}, {capture_id: "fresh"});
// @ts-expect-error capture and zoom are distinct coordinate routes
app.click([1, 2], {capture_id: "fresh", from_zoom: true});
// @ts-expect-error rightClick does not support capture bindings
app.rightClick([1, 2], {capture_id: "fresh"});
// @ts-expect-error rightClick modifiers require pixels
app.rightClick({name: "fixture"}, {modifiers: ["ctrl"]});
// @ts-expect-error doubleClick has no modifier support in DTW
app.doubleClick([1, 2], {modifiers: ["ctrl"]});
// Windows-only zoom options; macOS rejects these at runtime.
app.doubleClick([1, 2], {from_zoom: true});
app.rightClick([1, 2], {from_zoom: true});
// @ts-expect-error typeText is not a screenshot-bound click
app.typeText("fixture", [1, 2], {capture_id: "fresh"});

// Dynamic targets retain the options supported for both element and pixel input.
declare const dynamicTarget: Target;
app.click(dynamicTarget);
app.rightClick(dynamicTarget);
app.doubleClick(dynamicTarget);

// Native browser operations have their own schema; session authority stays private.
const browserDescription: ExecInput = { operation: "describe", name: "browser_pointer" };
const browserSnapshot: ExecInput = { operation: "get_browser_state", arguments: {
  target_id: "opaque", tab_id: "opaque-tab", snapshot_format: "semantic_v2", include_screenshot: true } };
const health: ExecInput = { operation: "health_report", arguments: { include: ["binary_version"] } };
// @ts-expect-error Runtime session must never be supplied by an agent.
const forgedBrowserSession: ExecInput = { operation: "browser_click", arguments: { session: "forged" } };
// @ts-expect-error No generic native tool passthrough.
const arbitraryNative: ExecInput = { operation: "set_config", arguments: {} };

// Native discovery and exact observation are typed separately from scripts.
const discoveryInputs: ExecInput[] = [
  {operation:"apps",arguments:{query:"Editor",running:true,limit:10}},
  {operation:"windows",arguments:{pid:123,title:"Settings"}},
  {operation:"windows",arguments:{app:"Editor",on_screen_only:true}},
  {operation:"observe",arguments:{pid:123,window_id:456,max_elements:80}},
  {operation:"app",pid:123,window_id:456,include_image:true},
];
// @ts-expect-error An observation must pin a window.
const missingWindow: ExecInput = {operation:"observe",arguments:{pid:123}};
// @ts-expect-error Window discovery requires one scope.
const unscopedWindows: ExecInput = {operation:"windows",arguments:{}};
