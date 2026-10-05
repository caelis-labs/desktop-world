from typing import Any, Generic, TypeVar, TypedDict
T = TypeVar("T")
class Fact(TypedDict, Generic[T], total=False):
    known: T
    status: str
    value: T
    reason: str
class UIObject(TypedDict, total=False):
    ref: str
    kind: str
    role: str
    name: Fact[str]
    app: str
    window: str
    parent: str
    uri: Fact[str]
    value_preview: Fact[str]
    states: dict[str, Fact[bool]]
    version: str
    geometry_version: str
    lifecycle: str
class Coverage(TypedDict, total=False):
    complete: bool
    dirty: bool
    truncated: bool
    continuation: str
    unavailable_sources: list[str]
class Observation(TypedDict, total=False):
    objects: list[UIObject]
    coverage: Coverage
    seat: dict[str, Any]
    cursor: str
class Receipt(TypedDict, total=False):
    run_id: str
    outcome: str
    steps: list[dict[str, Any]]
    fault: dict[str, Any]
    seat_health: str
    input: dict[str, Any]
class Reply(TypedDict, total=False):
    id: str
    protocol: str
    world: str
    result: Any
    error: dict[str, Any]
class Grant(TypedDict, total=False):
    id: str
    application: str
    name: str
    window_title: str
    state: str
    reason: str
class GrantStatus(TypedDict):
    turn: str
    grants: list[Grant]
