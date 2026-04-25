# servo-fetch Tool Guide

## Pagination pattern

Large pages are truncated at `max_length` (default 5000 characters). The response includes a hint:

```text
<content truncated. total_length=42000, next start_index=5000>
```

Fetch the next chunk:

```text
fetch(url: "https://...", start_index: 5000)
```

## Format selection

| Goal | Format |
| ---- | ------ |
| Read content, summarize, answer questions | `markdown` |
| Extract title, byline, excerpt, language | `json` |

## Troubleshooting

| Symptom | Solution |
| ------- | -------- |
| Empty content | Site may require JS features not yet supported by Servo. Try `execute_js` with `document.body.innerText` |
| Timeout | Increase timeout: `fetch(url: "...", timeout: 60)` |
| "access to private/local addresses is not allowed" | URL resolves to a private IP. Use a public URL |

## MCP configuration

```json
{
  "mcpServers": {
    "servo-fetch": {
      "command": "servo-fetch",
      "args": ["mcp"]
    }
  }
}
```
