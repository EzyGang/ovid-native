from collections.abc import Callable
from threading import Lock


class CallbackSubscription:
    def __init__(self, cancel: Callable[[], None]) -> None:
        self._cancel = cancel
        self._closed = False
        self._lock = Lock()

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        self._cancel()


class ListenerRegistry[Event]:
    def __init__(self) -> None:
        self._listeners: dict[int, Callable[[Event], None]] = {}
        self._lock = Lock()
        self._next_listener = 0

    def subscribe[Subscription: CallbackSubscription](
        self,
        listener: Callable[[Event], None],
        subscription_type: type[Subscription],
    ) -> Subscription:
        with self._lock:
            listener_id = self._next_listener
            self._next_listener += 1
            self._listeners[listener_id] = listener
        return subscription_type(lambda: self._remove(listener_id))

    def publish(self, event: Event) -> None:
        with self._lock:
            listeners = tuple(self._listeners.values())
        for listener in listeners:
            listener(event)

    def _remove(self, listener_id: int) -> None:
        with self._lock:
            self._listeners.pop(listener_id, None)
