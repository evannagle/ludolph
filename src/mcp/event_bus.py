"""Generic event bus with pub/sub support."""

import threading
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any


@dataclass
class Event:
    """An event in the bus."""

    id: int
    type: str
    timestamp: str
    data: dict[str, Any]
    source: str | None = None
    read_by: list[str] = field(default_factory=list)


class EventBus:
    """Thread-safe pub/sub event bus."""

    def __init__(self, max_events: int = 1000):
        self._events: deque[Event] = deque(maxlen=max_events)
        self._next_id = 1
        self._lock = threading.Lock()
        self._subscribers: set[str] = set()

    def subscribe(self, subscriber_id: str) -> None:
        """Register a subscriber."""
        with self._lock:
            self._subscribers.add(subscriber_id)

    def unsubscribe(self, subscriber_id: str) -> None:
        """Remove a subscriber."""
        with self._lock:
            self._subscribers.discard(subscriber_id)

    def publish(
        self,
        event_type: str,
        data: dict[str, Any],
        source: str | None = None,
    ) -> Event:
        """Publish an event to all subscribers."""
        with self._lock:
            event = Event(
                id=self._next_id,
                type=event_type,
                timestamp=datetime.now().isoformat(),
                data=data,
                source=source,
                read_by=[source] if source else [],
            )
            self._next_id += 1
            self._events.append(event)
            return event

    def receive(
        self,
        subscriber_id: str,
        since_id: int = 0,
        mark_read: bool = True,
    ) -> list[Event]:
        """Get unread events for a subscriber."""
        with self._lock:
            events = []
            for event in self._events:
                if event.id <= since_id:
                    continue
                if subscriber_id in event.read_by:
                    continue
                events.append(event)
                if mark_read:
                    event.read_by.append(subscriber_id)
            return events

    def get_recent(self, limit: int = 20) -> list[Event]:
        """Get recent events regardless of read status."""
        with self._lock:
            return list(self._events)[-limit:]


# Global instance
_bus: EventBus | None = None


def get_event_bus() -> EventBus:
    """Get or create the global event bus."""
    global _bus
    if _bus is None:
        _bus = EventBus()
    return _bus
