### Task 2.9 — Nemotron API research (Nemotron Ultra subagent)

**Files:**
- Create: `docs/decisions/05-nemotron-genai-api.md`

**Why this exists:** task 2.2 (genai adapter) needs to know the exact Nemotron endpoint URL, model identifier, and how `genai 0.6.5` surfaces the streaming events for it. This research produces a single document that the implementer can reference without leaving their worktree.

**Acceptance:**
- The doc cites 2+ official sources per claim (NVIDIA docs, model card, or `genai` source).
- Covers:
  1. **Endpoint URL.** Hosted (`integrate.api.nvidia.com/v1`) vs self-hosted NIM. What URL does task 2.2 use as the default?
  2. **Model identifier.** The exact string for Nemotron 3 Ultra on the hosted endpoint.
  3. **Tool-call streaming format.** Does the API return tool calls as deltas (multiple events) or as a single block? Cite the genai source.
  4. **Reasoning field.** Does `genai 0.6.5` surface a reasoning field for Nemotron? What's the field name?
  5. **Rate-limit response shape.** What HTTP status + body shape does Nemotron return on 429?
  6. **`genai` quirks.** Any Nemotron-specific behaviour in `genai::Client::exec_chat_stream` (e.g. required headers, special model flags, model-specific reasoning parsers)?
- Use **Nemotron Ultra** model (this is a research task, not an implementer task — it's cheap to be thorough).
- Save the document under `docs/decisions/05-nemotron-genai-api.md` with frontmatter matching Decision 01-04.

**Output:**
- A markdown document with one section per topic, each citing at least 2 sources inline.
- A short "Implications for task 2.2" section at the end summarising what the implementer needs to know.

**Routes:** dispatched via `claude -p` with model `nemotron` (or whatever the routing model is — see SOUL.md). Subagent has no code to write, only docs to read and synthesize.
