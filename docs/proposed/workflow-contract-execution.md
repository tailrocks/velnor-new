`plan_id` is `plan-<run-key>`. `detections`, `obligations`, `matrix.include`, and `task_ids` MUST be sorted as
defined by the architecture contract. `task_ids` contains every obligation; matrix `execute_task_ids` contains
only `execute` decisions. A baseline miss MUST populate the reason and broaden execution. A baseline proof
MUST identify the exact manifest/run/task/input digests. The plan job MUST publish `plan_id`, `run_key`, and
compact `matrix` JSON as named step outputs; the task job MUST consume that exact JSON and MUST NOT rediscover
stacks. The plan artifact MUST contain `plan.json` and `matrix.json`, where `matrix.json` is exactly
`{"include": [...]}`; the final job MUST validate byte-for-byte agreement after canonical JSON encoding.

## 5. Task execution

Rust task profiles follow the [task execution contract](task-execution-contract.md); future adapters own typed profiles.
