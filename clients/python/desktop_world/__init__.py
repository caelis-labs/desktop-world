"""Desktop World. HostSession owns authority; DesktopClient only sends desktop verbs."""
from .client import DesktopClient, DesktopError, HostSession, Plan, known
from .types import Coverage, Fact, Grant, GrantStatus, Observation, Receipt, Reply, UIObject
__all__ = ["DesktopClient", "DesktopError", "HostSession", "Plan", "known", "Coverage", "Fact", "Grant", "GrantStatus", "Observation", "Receipt", "Reply", "UIObject"]
