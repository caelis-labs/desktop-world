"""Shared fixed scripts and independent oracles for delayed form input."""
import json
import time

from app_client import require

CODE = "org:workspace is:open"
READY = "{title:'Review form ready',role:'text_field',text:'Review code',timeout_ms:4000}"
FILL = (f"const ready=await app.wait({READY}); "
        "if (!ready.elements.some(e => e.focused === true)) throw Error('Review code is not focused'); "
        f"await app.typeText({json.dumps(CODE)}); await app.keypress('ENTER');")
POSITIVE = "await app.click('Load review form'); " + FILL
FAULT = "await app.click('Load form with input fault'); " + FILL


def assert_business(state, *, fault=False):
    review = state["review"]
    require(review["loads"] == review["ready"] == (2 if fault else 1), review)
    require(review["submissions"] == 1 and review["submitted_code"] == CODE, review)
    require(review["dropped_keys"] == int(fault), review)
    require(review["visible_value"] == (CODE[1:] if fault else CODE), review)
    require(state["commits"] == state["saves"] == state["opens"] == 0, state)


def assert_fault(result):
    require(result["error"]["code"] == "text_delivery_unconfirmed", result)
    require(result["failure"]["operation"] == "scene.typeText", result)
    require(result["effects"]["dispatch"] == {"dispatched": 2}, result)
    require(any(ack.get("error") == "text_delivery_unconfirmed"
                for ack in result["effects"]["acknowledgements"]), result)


def assert_confirmed(result):
    require(any(ack.get("confirmed") == "text_present_in_focused_control"
                for ack in result["effects"]["acknowledgements"]), result)


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
    return {"full": full, "unchanged": unchanged, "small": small, "timer": timer,
            "stable": stable, "timeout": timeout, "positive": positive, "negative": negative, "pointer": pointer}
