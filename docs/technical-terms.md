# Technical Terms

| Term | Definition |
| --- | --- |
| Canonical JSON | JSON data with one specified byte representation. |
| Symbolic link | A file system entry that names another path. |
| Rust crate | A Rust source unit for one library or program. |
| Cargo feature | A named option that selects code or dependencies in a Rust package. |
| Rust package | A Cargo project with one manifest and one or more Rust crates. |
| Rust adapter | A Velnor component that reads Rust project data. |
| Task owner | A Cargo metadata value that selects the policy for the task list. |
| Runner selector | A configuration value that selects the runner for a job. |
| Named check | A task with its own name and workflow job. |
| Probe request | A journal record that tracks one requested resource probe and its state. |
| Inode | A number that identifies a file or directory within a file system. |
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
| Completion inbox | A journal table that keeps GitHub job messages for later processing. |
| Poll | A response from a GitHub queue that contains job messages. |
| Receipt | A record that confirms one decision or event. |
| Terminal state | A final job state that permits the controller to begin cleanup. |
| Publication snapshot | A journal record of the runner configuration that the controller published. |
| Acquired request | A job request that GitHub returned in response to `AcquireJobs`. |
| Physical reservation | A journal record that holds host capacity for one job request. |
| Guest resource sample | A measurement of CPU, memory, or storage from the selected guest engine. |
| Resource permit | An approval to start work once with no more than the allowed host capacity. |
| Worker pair | The runner container and its private Docker-in-Docker container for one job. |
| Recovery observation | A guest resource sample used to check an existing reservation during recovery. |
| Field | An item with a name in software data, such as a TOML value or a database column. |
| Accessor | A function that returns a stored or fixed software value. |
| Length-prefixed field | A byte sequence preceded by its length as an unsigned 64-bit integer with the most significant byte first. |
| Schema | The structure of a database, with its tables, columns, and checks. |
| Database trigger | A database operation that starts automatically when specified data changes. |
| Execution proof | A journal record used to check a job request and its worker pair. |
| Proof guard | A database trigger that stops a change to an execution proof when the data is not correct. |
| Digest | A value made from data that can show whether data changed. |
| Probe lease | A journal record that tracks an active resource-probe operation. |
| Cache key | A value that selects one cache entry. |
| Cache directive | Plan data that specifies the cache layers and identities permitted for a cache owner. |
| Cache family | A cache type with one key layer, schema, and path list. |
| Cache snapshot | The files saved in one cache entry. |
| Cache trust | An access rule that says which workflow runs can write to a cache. |
| Project tool version | The version of a tool that the project config declares. |
| Project tool resolver | A workflow step that reads project tool versions and sets environment values. |
| Plan output limit | The maximum size of output values that one job can send to other jobs. |
| Task binding digest | A digest that identifies a generated task command with project tool references. |
| Catalog tool | A tool that Velnor can set in generated workflows. |
| Mount handoff | The transfer of volume and directory identity records from the storage initializer to the controller. |
| Cleanup observer | A guest process that examines storage and process identities after cleanup. |
| Cleanup observer witness | A journal record with guest identity data recorded before cleanup. |
| Storage role | A named purpose for one runner storage volume or mount. |
| Named volume | A Docker volume with a name that a container mounts. |
| Docker root | The directory that the selected Docker engine uses to store Docker data. |
| Mount namespace | The Linux view of mounts used by one process. |
| Mount ID | The Linux identifier for one mount in a mount namespace. |
| statx | The Linux operation that reads file identity and mount data. |
| File system ID | The number that identifies a file system. |
| Device pair | The Linux major and minor device numbers. |
| Cleanup proof | A record that shows that the job’s Docker and guest resources are not present after cleanup. |
| Source context | A GitHub event and source ref, with pull request rules for the job. |
| Repository binding | A digest of one repository connection and its source policy. |
| Policy generation | A positive integer that identifies one policy version. |
| JIT configuration | Configuration data that registers one runner for one job. |
| Capacity probe | A probe that measures available host capacity. |
| Pre-JIT cleanup | Cleanup after Docker returns a final response to a container create request and before JIT configuration delivery. |
| Parser | Software that reads input data and checks its structure. |
| Cache owner | A generated job that owns cache layers for its matrix members. |
| Cache layer | One type of data stored in the qualification cache. |
| Matrix member | A plan entry that belongs to one generated job. |

| DDL | SQL statements that define or change database objects. |

| Physical attempt | An attempt to start an acquired request with one reservation. |

| Artifact | One or more files that a workflow produces. |

| Artifact attestation | A record that identifies an artifact and the workflow that produced it. |

| Attestation helper | A program that does a check of artifact attestations for the host. |

| ABI | Rules that software uses to communicate across a program boundary. |

| Runtime identity | A record of the runner, image, tools, and ABI facts used to bind cache keys. |
| Composite action | A GitHub Actions file that groups steps and declares inputs and outputs. |
| Typed identity | A Rust value whose type and fields identify one permitted software item. |
| Workflow phase | One specified group of operations in a workflow. |

| Future | A Rust value for an operation that can finish later. |

| Heap storage | Memory that a program allocates while it runs. |

| Physical retirement | Cleanup that stops a runner and releases its physical reservation after JIT configuration delivery. |
| Retirement claim | A journal record that permits physical retirement of one physical attempt until a specified time. |
| Retirement phase | One recorded step in physical retirement. |
| Retirement proof | A journal record that links runner absence, resource cleanup, and capacity checks for one physical retirement. |
| Retirement fence | A database rule that prevents a normal completion record while a retirement claim exists. |
| Runner registration | A GitHub service record that identifies one runner. |
| Runner response receipt | A record of one lookup or removal attempt that contains metadata and digests. |
| Runner absence receipt | A record of an exact lookup that reports no matching runner registration. |

| Cache census | A record of cache entries returned by an API query for one Git ref. |
| dbt project | A directory with dbt configuration, models, and related source files. |
| dbt profile | A dbt configuration that supplies connection settings for a data warehouse. |
| Data warehouse | A database that stores data for analysis. |
| Flow list | A YAML list written between square brackets. |
| Block list | A YAML list with one item on each line after a hyphen. |
