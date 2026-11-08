# Model provider contract

Each agent supplies its own persona, assigned position, model settings and audible-only context. Model output is unplayed until the audio path acknowledges it; it must never directly mutate the shared transcript.

The first adapter targets OpenAI-compatible streaming chat completions, including local Ollama's documented `/v1/chat/completions` path:
https://github.com/ollama/ollama/blob/main/docs/api/openai-compatibility.mdx

Operator configuration supplies the full endpoint; HTTPS is required except loopback. No tool execution is exposed by this adapter. Audience text travels in a separately labeled user-data field, never in the system instructions. Prompt boundaries supplement deterministic authorization; they cannot themselves guarantee a model ignores malicious text.

Tests use a local HTTP fixture, not an account or paid API. Live provider/model compatibility, latency and output quality remain unverified until measured.
