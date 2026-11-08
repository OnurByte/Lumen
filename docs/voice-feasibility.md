# Turkish voice feasibility

Candidate sources checked 2026-11-06:

- https://github.com/witetech/pocket-tts-turkish — package code Apache-2.0; Python API and built-in voices documented.
- https://huggingface.co/wite-tech/pocket-tts-turkish/blob/main/README.md — weights CC-BY-4.0; model card describes six synthetic voices, 24 kHz mono output and five emotion tags. Upstream speed claims are not Lumen measurements.

`uv` is installed. System Python is 3.14.8; use a worker-specific supported interpreter rather than installing ML dependencies into system Python. Model download, compatibility, streaming/cancellation behavior, voice quality, attribution requirements and hardware latency remain to be tested. No voice cloning is needed for v0.1; use the candidate's documented synthetic presets.

Benchmark Turkish names, dates, questions and emotional sentences. Record first PCM latency, synthesis duration, peak memory, sample format and interrupted-work cleanup. Listen to each preset before choosing the two show voices. Compare the other plan candidates before treating this candidate as selected.
