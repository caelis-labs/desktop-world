// DTW's small argument adapter around the unmodified, tag-pinned CUA prelude.
// This is a contract check, not a security boundary or an input retry layer.
function __dtw_click_api(native) {
  const pointer = new Set(["click", "double_click", "right_click"]);
  const keys = new Set(["cmd", "command", "super", "meta", "win", "shift",
                        "option", "alt", "ctrl", "control"]);
  const normalize = (tool, input = {}) => {
    if (!pointer.has(tool)) return input;
    if (!input || typeof input !== "object" || Array.isArray(input)) {
      throw new TypeError(`DTW ${tool}: options must be an object`);
    }
    const args = { ...input };
    const has = (key) => args[key] !== undefined;
    if (has("modifier") && has("modifiers")) {
      throw new TypeError(`DTW ${tool}: use only one of modifiers or modifier`);
    }
    if (has("modifiers")) {
      args.modifier = args.modifiers;
      delete args.modifiers;
    }
    if (has("modifier")) {
      if (!Array.isArray(args.modifier) ||
          args.modifier.some((key) => typeof key !== "string" || !keys.has(key))) {
        throw new TypeError(`DTW ${tool}: modifier(s) must be an array of supported key names`);
      }
      // macOS rejects this field; Windows accepts it but does not apply it.
      if (tool === "double_click") {
        throw new TypeError("DTW double_click: modifier(s) are unsupported; use click for modified input");
      }
      if (tool === "right_click" && args.modifier.length &&
          !(Number.isFinite(args.x) && Number.isFinite(args.y))) {
        throw new TypeError("DTW right_click: modifier(s) require a pixel point");
      }
    }
    if (tool !== "click") {
      for (const key of ["capture_id", "button"]) {
        if (has(key)) throw new TypeError(`DTW ${tool}: ${key} is unsupported; use click for screenshot-bound button input`);
      }
      if (has("from_zoom") && native.computer.target !== "windows") {
        throw new TypeError(`DTW ${tool}: from_zoom is supported only on Windows`);
      }
    } else if (has("capture_id")) {
      if (typeof args.capture_id !== "string" || !args.capture_id.trim() ||
          !Number.isFinite(args.x) || !Number.isFinite(args.y)) {
        throw new TypeError("DTW click: capture_id requires a nonempty ID and a pixel point");
      }
      if (args.from_zoom === true) {
        throw new TypeError("DTW click: capture_id and from_zoom cannot be combined");
      }
    }
    if (has("from_zoom") && typeof args.from_zoom !== "boolean") {
      throw new TypeError(`DTW ${tool}: from_zoom must be a boolean`);
    }
    return args;
  };
  const wrap = (app) => {
    if (!app || typeof app.click !== "function") return app;
    for (const [method, tool] of [["click", "click"], ["doubleClick", "double_click"],
                                   ["rightClick", "right_click"]]) {
      const original = app[method].bind(app);
      app[method] = async (target, options = {}) => {
        // Include the point for validation, leaving CUA to resolve real targets.
        const point = Array.isArray(target) ? { x: target[0], y: target[1] } : {};
        const args = normalize(tool, { ...point, ...options });
        if (!options || typeof options !== "object" || Array.isArray(options)) {
          throw new TypeError(`DTW ${tool}: options must be an object`);
        }
        if (Array.isArray(target)) {
          // Do not change the target or inject a new window/delivery mode.
          if (options.x === undefined) delete args.x;
          if (options.y === undefined) delete args.y;
        }
        return original(target, args);
      };
    }
    return app;
  };
  return Object.freeze({
    ...native,
    getApp: async (...args) => wrap(await native.getApp(...args)),
    launch: async (...args) => wrap(await native.launch(...args)),
    call: async (tool, args) => native.call(tool, normalize(tool, args)),
  });
}
