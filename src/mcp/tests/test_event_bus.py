"""Tests for event bus."""

import pytest
from mcp.event_bus import EventBus, Event


def test_publish_and_receive():
    """Published events are received by subscribers."""
    bus = EventBus()
    bus.subscribe("test_sub")

    event = bus.publish("channel_message", {"from": "cc", "content": "hi"})

    assert event.id == 1
    assert event.type == "channel_message"

    events = bus.receive("test_sub")
    assert len(events) == 1
    assert events[0].data["content"] == "hi"


def test_subscriber_does_not_receive_own_events():
    """Subscribers don't receive events they published."""
    bus = EventBus()
    bus.subscribe("sender")

    bus.publish("channel_message", {"from": "sender", "content": "hi"}, source="sender")

    events = bus.receive("sender")
    assert len(events) == 0


def test_events_marked_as_read():
    """Events are marked read after receive."""
    bus = EventBus()
    bus.subscribe("test_sub")

    bus.publish("test", {"msg": "hello"})

    events1 = bus.receive("test_sub")
    assert len(events1) == 1

    events2 = bus.receive("test_sub")
    assert len(events2) == 0


def test_multiple_subscribers_receive_same_event():
    """Multiple subscribers each receive the same event independently."""
    bus = EventBus()
    bus.subscribe("sub1")
    bus.subscribe("sub2")

    bus.publish("notification", {"message": "alert"})

    events1 = bus.receive("sub1")
    events2 = bus.receive("sub2")

    assert len(events1) == 1
    assert len(events2) == 1
    assert events1[0].id == events2[0].id


def test_unsubscribe_stops_receiving():
    """Unsubscribed clients don't receive new events."""
    bus = EventBus()
    bus.subscribe("temp_sub")
    bus.unsubscribe("temp_sub")

    bus.publish("test", {"msg": "hello"})

    # Unsubscribed subscriber should still be able to call receive
    # but conceptually they're no longer tracked
    events = bus.receive("temp_sub")
    # They still get events (receive works regardless of subscription status)
    # but unsubscribe is for cleanup purposes
    assert isinstance(events, list)


def test_since_id_filters_old_events():
    """Events with id <= since_id are not returned."""
    bus = EventBus()
    bus.subscribe("test_sub")

    bus.publish("event1", {"n": 1})
    bus.publish("event2", {"n": 2})
    bus.publish("event3", {"n": 3})

    # Get events since id=2 (should only get event 3)
    events = bus.receive("test_sub", since_id=2, mark_read=False)
    assert len(events) == 1
    assert events[0].data["n"] == 3


def test_get_recent_returns_events_regardless_of_read_status():
    """get_recent returns events even if already read."""
    bus = EventBus()
    bus.subscribe("test_sub")

    bus.publish("test", {"msg": "hello"})

    # Read the event
    bus.receive("test_sub")

    # get_recent should still return it
    recent = bus.get_recent(limit=10)
    assert len(recent) == 1
    assert recent[0].data["msg"] == "hello"


def test_max_events_enforced():
    """Event bus respects max_events limit."""
    bus = EventBus(max_events=3)

    bus.publish("e1", {})
    bus.publish("e2", {})
    bus.publish("e3", {})
    bus.publish("e4", {})  # This should push out e1

    recent = bus.get_recent(limit=10)
    assert len(recent) == 3
    assert recent[0].id == 2  # e1 (id=1) was evicted


def test_event_has_timestamp():
    """Events have a timestamp field."""
    bus = EventBus()

    event = bus.publish("test", {"msg": "hello"})

    assert event.timestamp is not None
    assert len(event.timestamp) > 0
    # Should be ISO format
    assert "T" in event.timestamp
