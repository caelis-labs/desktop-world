/** DTW script v1 authoring aid. `describe.script` is the live contract.
 * Submit JavaScript, not TypeScript: these declarations do not add VM globals.
 * One trusted app context; await calls sequentially. Never retry an unknown
 * effect automatically. Choose targets from returned refs and read back effects.
 */
type JSONValue = null | boolean | number | string | JSONValue[] | { [key: string]: JSONValue };
type WindowRef = string;
type ObservationRef = string;
type ElementRef = string;
interface ProtocolError {
  code: string;
  message: string;
  /** Diagnostic data only; never authority or a retry instruction. */
  details?: JSONValue;
}
interface Coverage {
  scope: "same_process_standard_windows";
  status: "available" | "empty_within_scope";
  complete_within_scope: true;
  candidate_count: number;
  supported_count: number;
  excluded_count: number;
  excludes: ["helper_processes", "non_standard_windows", "non_default_layers"];
}
interface InventoryDiagnostics {
  provider: string;
  counters: Record<string, number>;
  error_codes: number[];
}
interface Inventory {
  windows: { window_ref: WindowRef; title: string; visible: boolean }[];
  coverage: Coverage;
  diagnostics: InventoryDiagnostics;
}
interface SemanticObservation {
  window_ref: WindowRef;
  observation_ref: ObservationRef;
  kind: "semantic";
  elements: { element_ref: ElementRef; label: string; can_press: boolean }[];
}
interface PixelObservation {
  window_ref: WindowRef;
  observation_ref: ObservationRef;
  kind: "pixels";
  /** PNG bytes stay outside the VM; only the final capture is attached by MCP. */
  image: { width: number; height: number; coordinate_space: "image_pixels" };
}
type EffectEvidence =
  | { kind: "unverified" }
  | { kind: "confirmed"; postcondition: string }
  | { kind: "mismatch"; expected: string; observed: string };
interface ActionOutcome {
  dispatch: "not_dispatched" | "dispatched" | "unknown";
  evidence: EffectEvidence;
  error: ProtocolError | null;
}
interface ActionReceipt {
  outcome: ActionOutcome;
  provider_outcome: ActionOutcome | null;
  interruption: ProtocolError | null;
}
interface App {
  /** Requires observe. Replaces window refs and retires the current observation.
   * Unproven inventory throws code=window_inventory_unproven, with coverage
   * status=unproven/complete_within_scope=false and diagnostics in error.details.
   * A proven empty result means empty within this scope, not that the app has no windows.
   */
  windows(): Promise<Inventory>;
  /** Requires observe. Replaces the current semantic or pixel observation. */
  observe(args: { window_ref: WindowRef }): Promise<SemanticObservation>;
  /** Requires capture. Replaces the current semantic or pixel observation. */
  capture(args: { window_ref: WindowRef }): Promise<PixelObservation>;
  /** Requires press; fresh semantic observation, can_press=true. Consumes the
   * binding on Core action admission. An error throws; its receipt remains in
   * script.steps. Read back success independently of dispatch.
   */
  press(args: { window_ref: WindowRef; observation_ref: ObservationRef; element_ref: ElementRef }): Promise<ActionReceipt>;
  /** Requires click; fresh pixel observation. Coordinates are finite image
   * pixels with 0 <= x < width and 0 <= y < height. Consumes the admitted binding.
   */
  click(args: { window_ref: WindowRef; observation_ref: ObservationRef; x: number; y: number }): Promise<ActionReceipt>;
}
declare const app: Readonly<App>;
/** Modify fields; the root cannot be replaced. Commit only on successful script
 * completion before its deadline. No rollback of desktop effects on failure.
 * State and result: strict JSON, 64 KiB each, depth <=64, nodes <=32768.
 * Default/maximum execution: 5000 ms and 32 bridge attempts. All calls share the
 * earlier script/grant deadline. Timeout/cancel/expiry seals the context; no
 * timers, modules, OS APIs, concurrent Host calls or authority parameters exist.
 */
declare const state: Record<string, JSONValue>;
