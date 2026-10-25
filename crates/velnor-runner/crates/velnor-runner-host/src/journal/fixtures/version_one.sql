CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0, dind_id TEXT, worker_volume TEXT);
PRAGMA user_version = 1;
INSERT INTO intents (id, kind, subject, state, cleanup_proven) VALUES (1, 'launch', 'legacy-row', 'pending', 0);
