---
status: accepted
---

# Build all agent code with Rig

All of Whetstone's AI work is built with the Rig framework (the `rig` crate, which bundles `rig-core` and `rig-agent`). That covers question generation, explanations and hints through the generator, and decisions from Jev. We don't write HTTP calls by hand or use another SDK. We chose Rig for two reasons: it gives one provider abstraction, so direct Anthropic and OpenAI support can be added later without rewriting agent code, and it provides typed output and tool calling out of the box.

## Consequences

- Rig's OpenRouter chat provider can't call Jev, because Jev is a decisions API rather than a chat model. Reaching Jev means using Rig's experimental `rig-typesafeai` crate, configured for OpenRouter's `/api/v1/systemone` endpoint. In Rig 0.44.0 (7 October 2026) that path can't decode Jev's replies (rig#2782). The fix (rig#2800) is only on Rig's development branch, so the path must be verified on a release before anything relies on it.
- When Rig lacks something, raise it as a decision. Don't route around it with direct calls.
