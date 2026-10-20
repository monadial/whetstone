---
status: accepted
---

# Build all agent code with Rig

All of Whetstone's AI work is built with the Rig framework (the `rig` crate, which bundles `rig-core` and `rig-agent`). That covers question generation, explanations and hints through the generator, and decisions from Jev. We don't write HTTP calls by hand or use another SDK. We chose Rig for two reasons: it gives one provider abstraction, so direct Anthropic and OpenAI support can be added later without rewriting agent code, and it provides typed output and tool calling out of the box.

## Consequences

- Rig's OpenRouter chat provider can't call Jev, because Jev is a decisions API rather than a chat model. Reaching Jev means using Rig's experimental `rig-typesafeai` crate, through the `rig` crate's `typesafeai` feature, configured for OpenRouter's `/api/v1/systemone` endpoint. Rig 0.44.0 couldn't decode Jev's replies there (rig#2782). Rig 0.45.0 (19 October 2026) can (rig#2800).
- Rig is pinned to an exact version and upgraded on purpose, because its releases so far have each broken the API. Rig 0.45 needs Rust 1.97.1.
- In 0.45, `rig-typesafeai` can't send OpenRouter's `provider` routing flags, so a Jev request can't yet require zero-data retention. Until a Rig release can, Jev requests go without the flags, a temporary exception to the rule that every request carries them. That rests on TypeSafe being Jev's only provider on OpenRouter, with no prompt retention and no training (checked 20 October 2026). If that changed, Jev requests would still be sent rather than refused. The setting is requested upstream (rig#2871). We don't pin a Rig git revision or a fork to get it sooner.
- Jev is checked on every pull request against recorded OpenRouter replies. It's also checked live in a keyed end-to-end job that runs when Rig is upgraded, when the AI crate changes, and before a release.
- When Rig lacks something, raise it as a decision. Don't route around it with direct calls.
