from __future__ import annotations
import asyncio
import copy
import json
import os
import subprocess
from typing import Any, Callable
from .types import GrantStatus, Observation, Receipt, Reply

VERBS = {"observe", "read", "sync", "act", "capture", "get", "cancel"}
class DesktopError(RuntimeError):
    def __init__(self, fault: dict[str, Any], reply: Reply | None = None):
        super().__init__(fault.get("message", fault.get("code", "desktop_error")))
        self.code = fault.get("code", "desktop_error")
        self.fault, self.reply = fault, reply
        self.receipt = (reply or {}).get("result")
def known(fact: Any) -> Any:
    if not isinstance(fact, dict):
        return fact
    if "known" in fact:
        return fact["known"]
    if fact.get("status") == "known" and "value" in fact:
        return fact["value"]
    raise DesktopError({"code": "fact_unavailable", "message": "Field is unknown, unsupported, redacted or unrequested."})
def _target(value: str | dict[str, Any]) -> dict[str, Any]:
    return {"ref": value} if isinstance(value, str) else copy.deepcopy(value)
class Plan:
    """Records local steps. act() submits once; delivered input cannot be rolled back."""
    def __init__(self):
        self.steps: list[dict[str, Any]] = []
    def add(self, step: dict[str, Any]) -> Plan:
        if len(self.steps) >= 16:
            raise ValueError("A native plan has at most 16 steps.")
        self.steps.append({"id": f"s{len(self.steps)+1}", **copy.deepcopy(step)})
        return self
    def focus(self, ref: str) -> Plan:
        return self.add({"op": "focus", "target": _target(ref)})
    def bind_focus(self, name: str, within: str) -> dict[str, str]:
        self.add({"op": "bind_focus", "bind_focus": {"name": name, "within": within}})
        return {"bound": name}
    def bind(self, name: str, locator: dict[str, Any]) -> dict[str, str]:
        self.add({"op": "bind", "bind": {"name": name, "locator": locator, "require_unique": True}})
        return {"bound": name}
    def press(self, ref: str | dict[str, Any], key: str, modifiers: list[str] | None = None) -> Plan:
        return self.add({"op": "keyboard.press", "target": _target(ref), "press": {"key": key, "modifiers": modifiers or []}})
    def type(self, ref: str | dict[str, Any], text: str) -> Plan:
        return self.add({"op": "keyboard.type_text", "target": _target(ref), "type_text": {"text": text}})
    def set(self, ref: str, text: str) -> Plan:
        return self.add({"op": "set_value", "target": _target(ref), "set_value": {"text": text}})
    def invoke(self, ref: str) -> Plan:
        return self.add({"op": "invoke", "target": _target(ref)})
    def click(self, ref: str | dict[str, Any], *, button: str = "left", count: int = 1) -> Plan:
        return self.add({"op": "pointer.click", "target": _target(ref), "click": {"button": button, "count": count}})
    def build(self, **options: Any) -> dict[str, Any]:
        return {**options, "steps": copy.deepcopy(self.steps)}

class _Transport:
    def __init__(self, process: asyncio.subprocess.Process, hello: dict[str, Any]):
        self.process, self.hello = process, hello
        self.records: dict[str, tuple[str, asyncio.Future[Reply]]] = {}
        self.sequence = 0
        self.closed = False
        self.reader = asyncio.create_task(self._read())
    async def _read(self) -> None:
        try:
            while True:
                line = await self.process.stdout.readline()
                if not line:
                    raise DesktopError({"code": "session_disconnected", "message": "Session exited; preserve receipts and do not replay."})
                reply = json.loads(line)
                record = self.records.get(reply.get("id"))
                if not record:
                    raise DesktopError({"code": "invalid_reply", "message": "Unexpected session reply; effects may be unknown."})
                if not record[1].done():
                    record[1].set_result(reply)
        except (Exception, asyncio.CancelledError) as error:
            self.closed = True
            for _, future in self.records.values():
                if not future.done():
                    future.set_exception(error)
            if self.process.stdin:
                self.process.stdin.close()
    async def request(self, channel: str, op: str, args: Any = None, request_id: str | None = None) -> Reply:
        if self.closed:
            raise DesktopError({"code": "session_closed", "message": "Do not restart to recover uncertain input."})
        if channel == "desktop" and op not in VERBS:
            raise ValueError("Authorization belongs to HostSession, not the desktop channel.")
        if request_id is None:
            self.sequence += 1
            request_id = f"py-{self.sequence}"
        body = json.dumps({"id": request_id, "channel": channel, "op": op, "args": {} if args is None else args}, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        record = self.records.get(request_id)
        if record and record[0] != body:
            raise DesktopError({"code": "request_conflict", "message": "Reuse ID only with identical arguments."})
        if record is None:
            if len(self.records) >= 4096:
                raise DesktopError({"code": "resource_exhausted", "message": "Retain receipts before ending session."})
            future = asyncio.get_running_loop().create_future()
            future.add_done_callback(lambda f: f.exception() if not f.cancelled() else None)
            record = (body, future)
            self.records[request_id] = record
            self.process.stdin.write((body + "\n").encode("utf-8"))
            await self.process.stdin.drain()
        # Caller cancellation never destroys the original response record.
        return await asyncio.shield(record[1])
    async def close(self) -> None:
        self.closed = True
        self.process.stdin.close()
        try:
            await asyncio.wait_for(self.process.wait(), 3)
        except asyncio.TimeoutError:
            self.process.kill()
            await self.process.wait()
            await self.reader
            raise DesktopError({"code":"close_incomplete","message":"Forced termination does not prove native cleanup."})
        await self.reader
        if self.process.returncode != 0:
            raise DesktopError({"code":"close_incomplete","message":"Session exited unsuccessfully; native cleanup is not confirmed."})

class DesktopClient:
    def __init__(self, transport: _Transport):
        self._transport = transport
        self._queries: dict[int, tuple[Observation, dict[str, Any]]] = {}
    async def _call_reply(self, op: str, args: Any = None, *, request_id: str | None = None) -> Reply:
        try:
            reply = await self._transport.request("desktop", op, args, request_id)
        except asyncio.CancelledError:
            # Ending authority stops subsequent input; it does not erase prior effects.
            await asyncio.shield(self._transport.request("host", "end_turn"))
            raise
        if reply.get("error"):
            raise DesktopError(reply["error"], reply)
        return reply
    async def call(self, op: str, args: Any = None, *, request_id: str | None = None) -> Any:
        return (await self._call_reply(op, args, request_id=request_id)).get("result")
    async def observe(self, args: dict[str, Any] | str | None = None) -> Observation:
        request = {"scope": {"desktop": True}, "projection": "summary", "fields": ["name", "role", "app", "window"], "budget": {"max_results": 32, "max_output_bytes": 8192}, **({"scope": {"refs": [args]}} if isinstance(args, str) else args or {})}
        result = await self.call("observe", request)
        self._queries[id(result)] = (result, copy.deepcopy(request))
        if len(self._queries) > 64:
            self._queries.pop(next(iter(self._queries)))
        return result
    async def outline(self, ref: str, **options: Any) -> Observation:
        return await self.observe({"projection": "outline", "fields": ["name", "role"], **options, "scope": {"refs": [ref]}, "budget": {"max_depth": 4, "max_results": 32, "max_output_bytes": 8192, **options.get("budget", {})}})
    async def find(self, within: str, locator: dict[str, Any], **options: Any) -> Observation:
        return await self.outline(within, **{**options, "budget": {"max_depth": 12, **options.get("budget", {})}, "match": {**locator, "within": within}})
    async def next(self, observation: Observation) -> Observation:
        record = self._queries.get(id(observation))
        continuation = observation.get("coverage", {}).get("continuation")
        if record is None or record[0] is not observation or not continuation:
            raise ValueError("Use the original observation with native continuation.")
        return await self.observe({**record[1], "continuation": continuation})
    def plan(self) -> Plan:
        return Plan()
    async def act(self, plan: Plan | list[dict[str, Any]], *, request_id: str | None = None, **options: Any) -> Receipt:
        args = plan.build(**options) if isinstance(plan, Plan) else {**options, "steps": [{"id": f"s{i+1}", **s} for i, s in enumerate(plan)]}
        reply = await self._call_reply("act", args, request_id=request_id)
        receipt = reply["result"]
        if receipt.get("outcome") != "completed":
            raise DesktopError({"code": "action_not_completed", "message": "Inspect original receipt; do not replay."}, reply)
        return receipt
    async def read(self, ref: str, **options: Any) -> Any:
        return await self.call("read", {**options, "target": ref})
    async def sync(self, cursor: str, **options: Any) -> Any:
        return await self.call("sync", {**options, "cursor": cursor})
    async def capture(self, **args: Any) -> Any:
        return await self.call("capture", args)
    async def get(self, run_id: str) -> Receipt:
        return await self.call("get", {"run_id": run_id})
    async def cancel(self, run_id: str) -> Receipt:
        return await self.call("cancel", {"run_id": run_id})
    async def reconcile(self, request_id: str) -> Reply:
        record = self._transport.records.get(request_id)
        if record is None:
            raise ValueError("No original request with this ID.")
        return await asyncio.shield(record[1])
    async def set(self, ref: str, text: str, *, request_id: str | None = None) -> Receipt:
        return await self.act(self.plan().set(ref, text), request_id=request_id)
    async def invoke(self, ref: str, *, request_id: str | None = None) -> Receipt:
        return await self.act(self.plan().invoke(ref), request_id=request_id)

class HostSession:
    def __init__(self, transport: _Transport):
        self._transport = transport
        self.desktop = DesktopClient(transport)
        self.hello = transport.hello
    @classmethod
    async def start(cls, helper: str, *, input_mode: str = "cooperative", input_policy: str | None = None, write_apps: list[str] | None = None, write_app_windows: list[str] | None = None, assets_dir: str | None = None, audit: str | None = None, audit_mode: str | None = None, owner_file: str | None = None) -> HostSession:
        args = [helper, "session", "--input-mode", input_mode]
        for flag, value in [("--input-policy", input_policy), ("--assets-dir", assets_dir), ("--audit", audit), ("--audit-mode", audit_mode), ("--session", owner_file)]:
            if value is not None:
                args += [flag, value]
        for name in write_apps or []:
            args += ["--write-app", name]
        for title in write_app_windows or []:
            args += ["--write-app-window", title]
        process = await asyncio.create_subprocess_exec(*args, stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, limit=2**21,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        try:
            hello = json.loads(await asyncio.wait_for(process.stdout.readline(), 15))
            if hello.get("protocol") != "desktop-world/session-v0.1" or "dynamic_app_grants" not in hello.get("features", []):
                raise DesktopError({"code": "incompatible_helper", "message": "Use the matching RC dtw helper."})
            return cls(_Transport(process, hello))
        except BaseException:
            process.stdin.close()
            try:
                await asyncio.wait_for(process.wait(), 3)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()
            raise
    async def _owner(self, op: str, args: dict[str, Any] | None = None, request_id: str | None = None) -> Any:
        reply = await self._transport.request("host", op, args, request_id)
        if reply.get("error"):
            raise DesktopError(reply["error"], reply)
        return reply.get("result")
    async def grant(self, application: str, *, request_id: str | None = None) -> Any:
        return await self._owner("grant", {"application": application}, request_id)
    async def declare(self, name: str) -> Any:
        return await self._owner("declare", {"name": name})
    async def declare_window(self, window_title: str) -> Any:
        return await self._owner("declare", {"window_title": window_title})
    async def revoke(self, application: str) -> Any:
        return await self._owner("revoke", {"application": application})
    async def revoke_grant(self, grant_id: str) -> Any:
        return await self._owner("revoke", {"grant_id": grant_id})
    async def grants(self) -> GrantStatus:
        return await self._owner("grants")
    async def begin_turn(self, turn: str) -> Any:
        return await self._owner("begin_turn", {"turn": turn})
    async def end_turn(self) -> Any:
        return await self._owner("end_turn")
    async def close(self) -> None:
        await self._transport.close()
    async def __aenter__(self) -> HostSession:
        return self
    async def __aexit__(self, *_: Any) -> None:
        await self.close()
