//! Schema 6 fact tables for the read families. The `task_headers` blobs stay the served
//! bytes; these tables hold only what reads filter, sort, bind or compute.
pub(crate) const FACTS_SCHEMA: &str = "
CREATE TABLE task_facts(
  id TEXT PRIMARY KEY CHECK(typeof(id)='text' AND length(id) BETWEEN 1 AND 64),
  valid INTEGER NOT NULL CHECK(valid IN(0,1)),
  backend TEXT, repo_path TEXT, session_id TEXT, status TEXT, exit_code INTEGER,
  started_at TEXT, started_at_us INTEGER, finished_at TEXT, finished_at_us INTEGER,
  output_drained INTEGER CHECK(output_drained IN(0,1)),
  title TEXT, model TEXT, continuation_of TEXT, chain_root TEXT,
  CHECK((valid=1 AND backend IS NOT NULL AND repo_path IS NOT NULL AND session_id IS NOT NULL
         AND status IS NOT NULL AND started_at IS NOT NULL AND started_at_us IS NOT NULL
         AND output_drained IS NOT NULL
         AND (finished_at IS NULL)=(finished_at_us IS NULL))
     OR (valid=0 AND backend IS NULL AND repo_path IS NULL AND session_id IS NULL
         AND status IS NULL AND exit_code IS NULL AND started_at IS NULL AND started_at_us IS NULL
         AND finished_at IS NULL AND finished_at_us IS NULL AND output_drained IS NULL
         AND title IS NULL AND model IS NULL AND continuation_of IS NULL AND chain_root IS NULL))
);
CREATE INDEX facts_continuation ON task_facts(continuation_of);
CREATE INDEX facts_finished ON task_facts(finished_at_us,id);
CREATE INDEX facts_started ON task_facts(started_at_us,id);
CREATE INDEX facts_session ON task_facts(session_id,started_at_us DESC,id DESC);
CREATE INDEX facts_damaged ON task_facts(id) WHERE valid=0;
CREATE INDEX facts_unobserved ON task_facts(started_at_us DESC,id DESC) WHERE valid=1 AND (status='running' OR exit_code IS NULL);
CREATE TABLE task_processes(
  task TEXT PRIMARY KEY CHECK(length(task) BETWEEN 1 AND 64),
  pid INTEGER NOT NULL CHECK(pid>1),
  start_seconds INTEGER, start_micros INTEGER, boot_uuid TEXT, pgid INTEGER,
  CHECK((start_seconds IS NULL)=(start_micros IS NULL) AND (start_seconds IS NULL)=(boot_uuid IS NULL))
);
CREATE INDEX processes_pid ON task_processes(pid);
CREATE INDEX processes_pgid ON task_processes(pgid);
CREATE TABLE task_prompts(
  task TEXT PRIMARY KEY CHECK(length(task) BETWEEN 1 AND 64),
  form TEXT NOT NULL CHECK(form IN('inline','sidecar')),
  inline_text TEXT, preview TEXT, claimed_sha256 TEXT, source_name TEXT,
  truncated INTEGER NOT NULL CHECK(truncated IN(0,1)), blob BLOB,
  CHECK((form='inline' AND inline_text IS NOT NULL AND preview IS NULL AND claimed_sha256 IS NULL
         AND source_name IS NULL AND truncated=0 AND blob IS NULL)
     OR (form='sidecar' AND inline_text IS NULL AND preview IS NOT NULL
         AND source_name IS NOT NULL AND truncated=1))
);
CREATE TABLE artifact_manifests(
  task TEXT PRIMARY KEY CHECK(length(task) BETWEEN 1 AND 64),
  header_valid INTEGER NOT NULL CHECK(header_valid IN(0,1)),
  revision TEXT NOT NULL CHECK(typeof(revision)='text' AND length(revision)=32)
);
CREATE TABLE artifact_items(
  task TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0),
  artifact_id TEXT NOT NULL, path TEXT NOT NULL, kind TEXT NOT NULL, note TEXT NOT NULL,
  registered_at TEXT NOT NULL, provenance_method TEXT NOT NULL,
  device TEXT NOT NULL, inode TEXT NOT NULL, size INTEGER NOT NULL CHECK(size>=0),
  mtime_ns TEXT NOT NULL,
  PRIMARY KEY(task,ordinal)
);
";
