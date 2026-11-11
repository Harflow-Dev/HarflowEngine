"""Runs the reference engine's `interaction_events.page` over a Python state layout.

This file is executed by python3 in a subprocess with the reference sources (HarflowMCP commit
0ac7e3c, extracted from git) on PYTHONPATH. It reads {"tasks": [...], "limits": [...]} on stdin,
writes the tasks with the reference store, walks the feed at each limit and prints the pages.
"""

import json
import os
import sys

from harflow_mcp import interaction_events, store
from harflow_mcp.state_layout import resolve_layout


def main():
    request = json.load(sys.stdin)
    layout = resolve_layout()
    log_dir = layout.tasks_directory
    for spec in request["tasks"]:
        store.write(log_dir, store.TaskRecord(**spec))
    walks = {}
    for limit in request["limits"]:
        pages, cursor = [], None
        for _ in range(500):
            page = interaction_events.page(log_dir, cursor, limit)
            pages.append(page)
            cursor = page["next_cursor"]
            if cursor is None:
                break
        walks[str(limit)] = pages
    json.dump({"walks": walks, "files": sorted(os.listdir(log_dir))[:3]}, sys.stdout)


main()
