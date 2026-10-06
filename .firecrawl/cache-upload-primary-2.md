Parameter location: requestBody
Schema:
```json
{
  "type": "object",
  "nullable": true,
  "properties": {
    "enable_debug_logging": {
      "type": "boolean",
      "default": false,
      "description": "Whether to enable debug logging for the re-run."
    },
    "enable_debugger": {
      "type": "boolean",
      "default": false,
      "description": "Whether to enable the debugger for the re-run of this job."
    }
  }
}
```