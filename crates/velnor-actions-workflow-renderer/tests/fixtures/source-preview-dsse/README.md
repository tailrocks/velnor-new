# Synthetic preview signature fixture

The hosted private fixture workflow signed one statement containing these exact
receipt, Linux, and Darwin subjects. The signatures and certificate identities
are genuine. SDK payloads and release or qualification records are synthetic;
never execute the payloads or admit this fixture through production policy.
`context.json` records the actual private signer, caller, and invocation.

`trusted-root.jsonl` contains the unchanged Sigstore public-good service keys
from the authenticated TUF target whose SHA-256 is
`6494e21ea73fa7ee769f85f57d5a3e6a08725eae1e38c755fc3517c9e6bc0b66`.
Its JSON was serialized onto one line for the GitHub CLI offline root reader.
This fixture does not establish production SDK publication or Mac qualification.
