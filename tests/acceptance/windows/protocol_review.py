"""Shared fixed scripts and independent oracles for delayed form input."""
import json
import time

from app_client import require

CODE = "org:workspace is:open"
READY = "{title:'Review form ready',window:'stable',role:'text_field',text:'Review code',timeout_ms:4000}"
FILL = (f"const ready=await app.wait({READY}); "
        "const field=ready.elements.find(e=>e.name==='Review code'); "
        "if (!ready.windows[0].title.includes('Review form ready') || "
        "!field || !field.id.startsWith(ready.windows[0].id+'.')) throw Error('Wait returned the wrong window'); "
        "await app.wait({title:'Review form ready',role:'button',text:'Submit review'}); "
        f"await app.typeText({{target:field.id,text:{json.dumps(CODE)}}}); await app.keypress('ENTER');")
POSITIVE = (
    "const filter={role:'button',text:'Load review form'}; "
    "const first=(await app.look(filter)).elements[0].id; "
    "await app.look({role:'text_field',text:'Document editor'}); "
    "const second=(await app.look(filter)).elements[0].id; "
    "if (first!==second) throw Error('Element ID changed across filtered views'); "
    "await app.click(first); const after=await app.look(); "
    "if (!after.windows.some(w=>w.title?.includes('Review form ready'))) throw Error('Readback preceded the ready form'); "
    "if (!after.windows.some(w=>w.title==='')) throw Error('Fixture overlay was not observed'); "
    + FILL + " return {id:first,destination:ready.windows[0].title};")
MISMATCH = (
    "const view=await app.wait({title:'Review form ready',role:'text_field',text:'Document editor'}); "
    "await app.typeText({target:view.elements[0].id,text:'must not reach either field'}); "
    "await app.keypress('ENTER');")
FAULT = "await app.click('Load form with input fault'); " + FILL
SUPPLEMENTARY_FAULT = FAULT.replace(json.dumps(CODE), json.dumps("A😀"))
RECOVERY = "await app.look(); await app.click('Load review form'); await app.typeText('implicit focus');"


def assert_business(state, *, fault=False):
    review = state["review"]
    require(review["loads"] == review["ready"] == (2 if fault else 1), review)
    require(review["submissions"] == 1 and review["submitted_code"] == CODE, review)
    require(review["dropped_keys"] == int(fault), review)
    require(review["visible_value"] == (CODE[1:] if fault else CODE), review)
    document_effects = {key: state[key] for key in ("commits", "saves", "opens")}
    require(not any(document_effects.values()), {"unexpected_document_effects": document_effects})
    require(state["text"] == "", "Document editor changed during the review task")


def assert_mismatch(result):
    require(result["error"]["code"] == "text_focus_mismatch", result)
    require("Review code" in result["error"]["message"], result)
    require(result["failure"]["operation"] == "scene.typeText", result)
    require(result["effects"]["dispatch"] == {"not_dispatched": 1}, result)


def assert_fault(result):
    require(result["error"]["code"] == "text_delivery_unconfirmed", result)
    require(result["failure"]["operation"] == "scene.typeText", result)
    require(result["effects"]["dispatch"] == {"dispatched": 2}, result)
    require(any(ack.get("error") == "text_delivery_unconfirmed"
                for ack in result["effects"]["acknowledgements"]), result)
    require(not result.get("shutdown_error") and not result.get("shutdown_errors"), result)


def assert_confirmed(result):
    require(any(ack.get("confirmed") == "text_present_in_focused_control"
                for ack in result["effects"]["acknowledgements"]), result)


def assert_recovery(state):
    review = state["review"]
    require(review["loads"] == review["ready"] == 4 and review["dropped_keys"] == 2, review)
    require(review["submissions"] == 1 and review["submitted_code"] == CODE, review)
    require(review["visible_value"] == "implicit focus", review)
    require(state["text"] == "" and state["commits"] == state["saves"] == state["opens"] == 0,
            "Unrelated document effect during recovery")


def direct(execute, state):
    full = execute("await app.look();")
    require(full["view"]["kind"] == "full", full)
    unchanged = execute("await app.look();")
    require(unchanged["view"] == {"kind": "unchanged"}, unchanged)
    small = execute("const v=await app.look({role:'button',text:'Load review form'}); return v.elements.length;")
    require(small["result"] == 1 and "view" not in small, small)
    next_view = execute("await app.look();")
    require(not next_view["view"].get("semantics", {}).get("query"), next_view)
    started = time.monotonic()
    timer = execute("await app.wait(80); return 'elapsed';")
    require(timer["result"] == "elapsed" and "view" not in timer and "effects" not in timer, timer)
    require(time.monotonic() - started >= .08, "Timer completed early")
    stable = execute("await app.wait({window:'stable',title:'DTW Computer Use Windows'});")
    require("effects" not in stable, stable)
    timeout = execute("await app.wait({title:'No such review window',timeout_ms:200});", True)
    require(timeout["error"]["code"] == "wait_timeout" and "effects" not in timeout, timeout)
    positive = execute(POSITIVE)
    assert_confirmed(positive)
    assert_business(state())
    same_id = execute("const v=await app.wait({title:'Review form ready',role:'button',text:'Load review form'});"
                      "return v.elements[0].id;")
    require(same_id["result"] == positive["result"]["id"], same_id)
    mismatch = execute(MISMATCH, True)
    assert_mismatch(mismatch)
    assert_business(state())
    negative = execute(FAULT, True)
    assert_fault(negative)
    assert_business(state(), fault=True)
    capture = execute("await app.capture({full:true});")
    image = capture["view"]["image"]
    x, y, width, height = image["bounds"]
    physical = state()["review"]["pointer"]
    require(physical is not None, "Fixture could not resolve the physical checkbox point")
    px, py = physical
    point = {"x": (px - x) / width, "y": (py - y) / height, "unit": "fraction"}
    pointer = execute("await app.click(" + json.dumps(point) + ");")
    require(pointer["effects"]["dispatch"] == {"dispatched": 1}, pointer)
    require(state()["reviewed"] and state()["checks"] == 1, state())
    assert_business(state(), fault=True)
    supplementary = execute(SUPPLEMENTARY_FAULT, True)
    assert_fault(supplementary)
    review = state()["review"]
    require(review["visible_value"] == "😀" and review["dropped_keys"] == 2
            and review["submissions"] == 1, review)
    recovered_view = execute("await app.look();")
    require("view" in recovered_view, recovered_view)
    implicit = execute(RECOVERY)
    assert_confirmed(implicit)
    assert_recovery(state())
    return {"full": full, "unchanged": unchanged, "small": small, "timer": timer,
            "stable": stable, "timeout": timeout, "positive": positive, "same_id": same_id, "mismatch": mismatch,
            "negative": negative, "pointer": pointer, "supplementary_fault": supplementary,
            "recovered_view": recovered_view, "implicit": implicit}
