# Technical Terms

| Term | Definition |
| --- | --- |
| Rust crate | A Rust source unit for one library or program. |
| Rust package | A Cargo project with one manifest and one or more Rust crates. |
| Rust adapter | A Velnor component that reads Rust project data. |
| Task owner | A Cargo metadata value that selects the policy for the task list. |
| Runner selector | A configuration value that selects the runner for a job. |
| Named check | A task with its own name and workflow job. |
| Journal | A database that keeps data about Velnor operations. |
| Migration | A change from one specified database structure to another. |
| Recursion | A function call that starts the same function directly or through other function calls. |
| Test fixture | Test data and code for a specified check. |
| Database | A set of related data that software keeps for later use. |
| Assigned job count | The value of Statistics.TotalAssignedJobs in a Scale Set response. |
| Available request | A job request in a JobAvailable message from GitHub. |
| Source approval | A checked decision that permits a specified source to use a runner. |
| Source receipt | A journal value that identifies one source approval. |
| Lifecycle event | A GitHub message about a job that starts or completes. |
| ACK | A reply that confirms receipt of a message. |
| Acquired request | A job request that GitHub returned in response to `AcquireJobs`. |
| Physical reservation | A journal record that holds host capacity for one job request. |
| Guest resource sample | A measurement of CPU, memory, or storage from the selected guest engine. |
| Resource permit | A one-use approval to start work within the host capacity limit. |
| Worker pair | The runner container and its private Docker-in-Docker container for one job. |
| Recovery observation | A guest resource sample used to check an existing reservation during recovery. |
