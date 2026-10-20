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
// @ts-expect-error doubleClick has no zoom support in DTW
app.doubleClick([1, 2], {from_zoom: true});
// @ts-expect-error typeText is not a screenshot-bound click
app.typeText("fixture", [1, 2], {capture_id: "fresh"});
