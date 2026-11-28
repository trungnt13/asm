import sys
import time
from pathlib import Path

wave, slot, total = map(int, sys.argv[1:])
ready = Path(".pgo-wave") / str(wave)
ready.mkdir(parents=True, exist_ok=True)
(ready / str(slot)).touch()
deadline = time.monotonic() + 120
while not all((ready / str(peer)).is_file() for peer in range(1, total + 1)):
    if time.monotonic() >= deadline:
        raise SystemExit("Parallel training wave did not start all children")
    time.sleep(0.25)
