import {
  createModels,
  fauxAssistantMessage,
  fauxProvider,
  fauxText,
  fauxThinking,
  fauxToolCall,
  type FauxProviderHandle,
  type Models,
} from "@earendil-works/pi-ai";

/** Model ref that resolves against a `makeFauxModels()` collection. */
export const FAUX_MODEL_REF = "faux/faux-1";

// Re-export the pi faux scripting helpers so @crow/daemon tests can script
// responses without taking a direct pi-ai dependency.
export { fauxAssistantMessage, fauxText, fauxThinking, fauxToolCall, type FauxProviderHandle };

/** A Models collection backed by the scripted faux provider. */
export function makeFauxModels(options?: { tokensPerSecond?: number }): {
  models: Models;
  faux: FauxProviderHandle;
} {
  const faux = fauxProvider({
    provider: "faux",
    models: [{ id: "faux-1", name: "Faux One", contextWindow: 64_000, maxTokens: 8_192 }],
    ...(options?.tokensPerSecond !== undefined ? { tokensPerSecond: options.tokensPerSecond } : {}),
  });
  const models = createModels();
  models.setProvider(faux.provider);
  return { models, faux };
}
