import json


def read_events(path):
    with open(path) as stream:
        return [json.loads(line) for line in stream]


def summarize(events):
    totals = {}
    for event in events:
        name = event["tool"]
        totals[name] = totals.get(name, 0) + event["duration_ms"]
    return totals
