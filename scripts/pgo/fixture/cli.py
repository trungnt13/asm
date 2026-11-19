import argparse
import json
from eventlog import read_events, summarize

parser = argparse.ArgumentParser()
parser.add_argument("path")
args = parser.parse_args()
print(json.dumps(summarize(read_events(args.path)), sort_keys=True))
