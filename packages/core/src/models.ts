import { createModels, type Api, type Model, type Models } from "@earendil-works/pi-ai";
import { builtinProviders } from "@earendil-works/pi-ai/providers/all";

/** Crow is NVIDIA-first; override per session or via `crowd --model`. */
export const DEFAULT_MODEL_REF = "nvidia/nemotron-3-ultra-550b-a55b" as const;

/** "provider/modelId" — the modelId may itself contain slashes; split on the first slash only. */
const MODEL_REF_PATTERN = /^([^/]+)\/(.+)$/;

export interface ParsedModelRef {
  provider: string;
  modelId: string;
}

export function parseModelRef(ref: string): ParsedModelRef {
  const match = MODEL_REF_PATTERN.exec(ref);
  if (!match) {
    throw new Error(`invalid model ref "${ref}" (expected "provider/modelId")`);
  }
  return { provider: match[1]!, modelId: match[2]! };
}

/** A Models collection with every built-in pi provider registered. */
export function createCrowModels(): Models {
  const models = createModels();
  for (const provider of builtinProviders()) {
    models.setProvider(provider);
  }
  return models;
}

/** Resolve "provider/modelId" to a concrete Model, or throw listing known providers. */
export function resolveModelRef(models: Models, ref: string): Model<Api> {
  const { provider, modelId } = parseModelRef(ref);
  const model = models.getModel(provider, modelId);
  if (!model) {
    const known = models
      .getProviders()
      .map((p) => p.id)
      .join(", ");
    throw new Error(`model not found: "${ref}" (known providers: ${known})`);
  }
  return model;
}
