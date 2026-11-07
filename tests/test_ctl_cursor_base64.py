"""Foreign cursors in several base64 spellings, with the verdict of the reference decoder.

Each cursor encodes a valid foreign cursor for task A and the default filter (event_pages.py of
HarflowMCP a426c5a: `parse_cursor`, which decodes with `base64.b64decode(cursor,
altchars=b"-_", validate=True)`). The verdicts were computed by running that function once, so
they are literals and the engine under test is never their source. A cursor the reference
accepts is well-formed and, being of another generation, is expired; one it rejects is invalid.
"""

import unittest

import seed_support as seeds
from history_support import HistoryCase
from seed_support import A

CASES = {'padded urlsafe': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ==',
                    'python': 'valid'},
 'padded two': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHgifQ==',
                'python': 'valid'},
 'stripped two': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHgifQ',
                  'python': 'invalid'},
 'padded one': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4In0=',
                'python': 'valid'},
 'stripped one': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4In0',
                  'python': 'invalid'},
 'no padding needed': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eCJ9',
                       'python': 'valid'},
 'standard alphabet': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJleHRyYSI6Ij4+Pj8/Pz4+Pj8/Pz4+Pj8/PyJ9',
                       'python': 'valid'},
 'urlsafe alphabet same payload': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJleHRyYSI6Ij4-Pj8_Pz4-Pj8_Pz4-Pj8_PyJ9',
                                   'python': 'valid'},
 'trailing garbage': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ==!!',
                      'python': 'invalid'},
 'trailing equals': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ===',
                     'python': 'invalid'},
 'trailing newline': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ==\n',
                      'python': 'invalid'},
 'inner space': {'cursor': 'eyJ2Ijoy '
                           'LCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ==',
                 'python': 'invalid'},
 'leading space': {'cursor': ' '
                             'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlfQ==',
                   'python': 'invalid'},
 'garbage after padding': {'cursor': 'eyJ2IjoyLCJnZW4iOiJweS1ldmVudHMtMSIsImRpciI6Im9sZGVyIiwidGFzayI6IjMwMzYxN2I5NzMwMjEwZWYzYzg2YzUyZGMyYWVjYzRkY2U1NGFhY2E2YWY4YzhiMGY0Y2VlYzllY2M1NGU1N2UiLCJkZXYiOjEsImlubyI6MiwicGF0aCI6InBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHAiLCJib3VuZGFyeSI6MywiYW5jaG9yIjoiYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImhlYWQiOiJoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoIiwiaGxlbiI6NjQsImZpbHRlciI6ImNlMzg5Y2VlMmY2ZjVhNGY0YTg5ZDljZjUzMjMxODRhYmNkZjk2OTk0YjdlZjkyYjkzODgxZjgyMzUzYjg0YjEiLCJyZXN5bmMiOmZhbHNlLCJzZXEiOjMsInNraXAiOmZhbHNlLCJwYWQiOiJ4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHgifQ==AAAA',
                           'python': 'invalid'}}


class Base64SpellingTests(HistoryCase):
    def setUp(self):
        super().setUp()
        self.seed("seed_world", {"world": seeds.world()})

    def verdict(self, name):
        case = CASES[name]
        code = "event_cursor_expired" if case["python"] == "valid" else "event_cursor_invalid"
        self.bad("task-events", A, "--cursor", case["cursor"], code=code)

    def test_every_spelling_gets_the_verdict_of_the_reference_decoder(self):
        for name in CASES:
            if name != "standard alphabet":
                with self.subTest(spelling=name):
                    self.verdict(name)

    def test_a_standard_alphabet_cursor_gets_the_verdict_of_the_reference_decoder(self):
        """The reference accepts `+` and `/` as well as `-` and `_`, so such a cursor is
        well-formed and expired, not invalid."""
        self.verdict("standard alphabet")

    def test_the_table_covers_padding_both_alphabets_and_trailing_garbage(self):
        python = {name: case["python"] for name, case in CASES.items()}
        self.assertEqual(python["standard alphabet"], "valid")
        self.assertEqual(python["urlsafe alphabet same payload"], "valid")
        self.assertEqual(python["stripped one"], "invalid")
        self.assertTrue(any(("+" in c["cursor"] or "/" in c["cursor"]) for c in CASES.values()))
