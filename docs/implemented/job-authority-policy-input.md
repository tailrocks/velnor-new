# Job authority policy input

`velnor-host connect` accepts one of two inputs. `--job-authority-inventory FILE` reads the schema-1 inventory from `velnor-actions authority-inventory`. The command also requires `--job-authority-generator-source-sha`, `--job-authority-generator-artifact-sha256`, and `--job-authority-generation`. The host checks each source context against the connection policy. It binds the result to that connection and validates the policy. The operator must review the generator source and artifact hashes. These hashes do not prove which release made the inventory. `--job-authority-policy FILE` accepts one complete policy JSON object.

Either input must add one policy for the requested connection and match its repository binding. Existing connections and policies remain unchanged. The host rejects invalid, too large, incomplete, duplicate, or mismatched input before it stores the connection token. The host does not create approval. The operator must review the inventory and its workflow source references.

The host validates the policy schema, content digests, all referenced workflows and reusable workflows, repository IDs, source contexts, and policy generation. These checks do not create approval. The operator must review the policy and its workflow source references before passing the file.

`connect` rejects an existing policy for the same connection. Jobs remain denied when no policy is present. Do not copy a policy from another repository or connection.
